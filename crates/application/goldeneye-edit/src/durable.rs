//! Crash-recoverable filesystem mutations coupled to targeted graph refreshes.

mod artifacts;
mod commit;
mod filesystem;
mod mutation;
mod recovery;
mod state;

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use goldeneye_domain::{
    ContentHash, FileId, Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath,
    SyntaxIdentityError,
};
use goldeneye_ports::{EditIndexer, EditRepository, EditSyntax, PortError};
use thiserror::Error;

use artifacts::{
    ArtifactPaths, TargetLease, changed_graph_identities, ensure_generation, graph_changes,
    join_relative, next_generation, planned_missing_parents, required_hash,
};
use filesystem::{
    cleanup_artifacts, ensure_file_hash, hard_link_new, hash_if_file, metadata, path_present,
    read_file, remove_empty_confined_directory, remove_if_exists, rename_new, sync_parent,
    validate_journal_artifacts, write_temp,
};

use crate::path_auth::{PathAuthorizationError, PathAuthorizer};
use crate::{
    EditDiagnostics, EditError, EditOperation, EditOptions, ParsePolicy, SourceDiff,
    TokenSizeMetadata,
};

/// Filesystem and persistence boundaries where deterministic tests can simulate a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    AfterJournal,
    BeforeWrite,
    AfterTemp,
    AfterBackup,
    AfterRename,
    DuringReindex,
    Cleanup,
}

/// Optional fault hook used by durability tests and embedders.
pub trait FaultInjector: Send + Sync {
    /// Returns an error to interrupt the operation at `point` without cleanup.
    ///
    /// # Errors
    ///
    /// Returns an implementation-defined message when the operation should stop at `point`.
    fn check(&self, point: FaultPoint) -> Result<(), String>;
}

#[derive(Debug)]
struct NoFault;

impl FaultInjector for NoFault {
    fn check(&self, _point: FaultPoint) -> Result<(), String> {
        Ok(())
    }
}

/// One structural edit request. The locator carries project, path, language, hash, and generation.
#[derive(Debug, Clone)]
pub struct DurableEditRequest {
    pub operation_id: String,
    pub locator: NodeLocator,
    pub operation: EditOperation,
    pub options: EditOptions,
}

/// One no-overwrite file creation request.
#[derive(Debug, Clone)]
pub struct DurableCreateRequest {
    pub operation_id: String,
    pub project_id: ProjectId,
    pub relative_path: ProjectRelativePath,
    pub language_id: LanguageId,
    pub source: Arc<[u8]>,
    pub expected_generation: Generation,
    pub parse_policy: ParsePolicy,
    pub create_parents: bool,
}

/// Compact graph delta for the targeted file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphChanges {
    pub added: usize,
    pub removed: usize,
    pub retained: usize,
}

/// Durable mutation output kept intentionally bounded for agent context efficiency.
#[derive(Debug, Clone)]
pub struct MutationResult {
    pub operation_id: String,
    pub project_id: ProjectId,
    pub relative_path: ProjectRelativePath,
    pub old_file_hash: Option<ContentHash>,
    pub new_file_hash: ContentHash,
    pub diff: SourceDiff,
    pub syntax_identities: Vec<NodeLocator>,
    pub changed_graph_identities: Vec<String>,
    pub graph_changes: GraphChanges,
    pub generation: Generation,
    pub diagnostics: EditDiagnostics,
    pub token_size: TokenSizeMetadata,
}

/// Resolution selected from actual on-disk hashes during startup recovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    CommittedNewSource,
    RestoredOriginalSource,
    RemovedIncompleteCreate,
    PreservedConflict,
}

/// Result for one journal row inspected during startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryEntry {
    pub operation_id: String,
    pub project_id: ProjectId,
    pub relative_path: ProjectRelativePath,
    pub resolved: bool,
    pub action: RecoveryAction,
    pub error: Option<String>,
}

/// Bounded startup recovery report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub entries: Vec<RecoveryEntry>,
}

/// Typed failures for authorization, stale identity, durable I/O, graph refresh, and recovery.
#[derive(Debug, Error)]
pub enum DurableEditError {
    #[error(transparent)]
    Path(#[from] PathAuthorizationError),
    #[error(transparent)]
    Repository(#[from] PortError),
    #[error(transparent)]
    Edit(#[from] EditError),
    #[error(transparent)]
    Identity(#[from] SyntaxIdentityError),
    #[error("project is not indexed: {0:?}")]
    ProjectNotFound(ProjectId),
    #[error("stale project generation: expected {expected:?}, actual {actual:?}")]
    StaleGeneration {
        expected: Generation,
        actual: Generation,
    },
    #[error("stored file identity is missing for {0:?}")]
    FileNotIndexed(ProjectRelativePath),
    #[error("stale source hash: expected {expected}, actual {actual}")]
    StaleSource {
        expected: ContentHash,
        actual: ContentHash,
    },
    #[error("project generation overflow for {0:?}")]
    GenerationOverflow(ProjectId),
    #[error("target already has an active mutation: {path}", path = .0.display())]
    TargetBusy(PathBuf),
    #[error("edit would not change source bytes")]
    NoContentChange,
    #[error("I/O failure while {operation} {path}: {source}", path = path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("fault injected at {point:?}: {message}")]
    InjectedFault { point: FaultPoint, message: String },
    #[error("targeted index refresh rejected durable source: {reason}")]
    RefreshRejected { reason: String },
    #[error("recovery material must be preserved for {operation_id}: {reason}")]
    RecoveryRequired {
        operation_id: String,
        reason: String,
    },
    #[error("journal recovery paths do not match operation {0}")]
    JournalPathMismatch(String),
    #[error("edit journal operation not found: {0}")]
    OperationNotFound(String),
}

/// Owns syntax planning, authorized filesystem mutation, journal recovery, and targeted indexing.
pub struct DurableEditService {
    index: Box<dyn EditIndexer>,
    journal: Box<dyn EditRepository>,
    syntax: Box<dyn EditSyntax>,
    authorizer: PathAuthorizer,
    fault_injector: Arc<dyn FaultInjector>,
}

impl DurableEditService {
    /// Opens the service and reconciles every incomplete journal row before returning.
    ///
    /// # Errors
    ///
    /// Returns a configuration/store error when roots cannot be authorized or the journal cannot
    /// be listed. Individual recovery conflicts are reported without preventing startup.
    pub fn open(
        index: impl EditIndexer + 'static,
        journal: impl EditRepository + 'static,
        syntax: impl EditSyntax + 'static,
        allowed_roots: Vec<PathBuf>,
    ) -> Result<(Self, RecoveryReport), DurableEditError> {
        let authorizer = PathAuthorizer::new(allowed_roots)?;
        let mut service = Self {
            index: Box::new(index),
            journal: Box::new(journal),
            syntax: Box::new(syntax),
            authorizer,
            fault_injector: Arc::new(NoFault),
        };
        let recovery = service.recover_incomplete()?;
        Ok((service, recovery))
    }

    /// Finds one project required by edit inspection workflows.
    ///
    /// # Errors
    ///
    /// Returns an error when the project registry cannot be read.
    pub fn indexed_project(
        &self,
        project: &ProjectId,
    ) -> Result<Option<goldeneye_domain::ProjectRecord>, DurableEditError> {
        Ok(self.journal.get_project(project)?)
    }

    /// Finds one file required by edit inspection workflows.
    ///
    /// # Errors
    ///
    /// Returns an error when the file registry cannot be read.
    pub fn indexed_file(
        &self,
        file: &FileId,
    ) -> Result<Option<goldeneye_domain::FileRecord>, DurableEditError> {
        Ok(self.journal.get_file(file)?)
    }

    pub fn set_fault_injector(&mut self, injector: Arc<dyn FaultInjector>) {
        self.fault_injector = injector;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use super::{DurableEditError, TargetLease};

    #[test]
    fn parallel_same_target_lease_conflicts_until_owner_releases() {
        let target = std::env::temp_dir().join("goldeneye-parallel-target.rs");
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let worker_target = target.clone();
        let worker_entered = Arc::clone(&entered);
        let worker_release = Arc::clone(&release);
        let worker = std::thread::spawn(move || {
            let _lease = TargetLease::acquire(&worker_target).expect("first target lease");
            worker_entered.wait();
            worker_release.wait();
        });
        entered.wait();
        assert!(matches!(
            TargetLease::acquire(&target),
            Err(DurableEditError::TargetBusy(_))
        ));
        release.wait();
        worker.join().expect("lease worker");
        TargetLease::acquire(&target).expect("lease after release");
    }
}
