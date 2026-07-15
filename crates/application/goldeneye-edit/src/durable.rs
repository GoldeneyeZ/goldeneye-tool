//! Crash-recoverable filesystem mutations coupled to targeted graph refreshes.

use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use goldeneye_domain::{
    ByteSpan, ContentHash, FileContext, FileId, Generation, LanguageId, NodeLocator, ProjectId,
    ProjectRelativePath, SyntaxIdentityError,
};
use goldeneye_index::{FileRefreshResult, FileRefreshStatus, IndexError, IndexService};
use goldeneye_store::{
    EditJournalRecord, EditOperationId, EditOperationKind, EditPhase, NewEditJournalRecord,
    StoreError,
};
use goldeneye_syntax::{GrammarProvider, LocatorError, SyntaxEngine, all_named_locators};
use thiserror::Error;

use crate::path_auth::{AuthorizedPath, PathAuthorizationError, PathAuthorizer, PathIntent};
use crate::{
    EditDiagnostics, EditError, EditOperation, EditOptions, ParsePolicy, SourceDiff,
    TokenSizeMetadata, plan_edit, validate_create_content,
};

static ACTIVE_TARGETS: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();

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
    Store(#[from] StoreError),
    #[error(transparent)]
    Edit(#[from] EditError),
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error(transparent)]
    Locator(#[from] LocatorError),
    #[error(transparent)]
    Identity(#[from] SyntaxIdentityError),
    #[error(transparent)]
    Syntax(#[from] goldeneye_syntax::SyntaxError),
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
}

/// Owns syntax planning, authorized filesystem mutation, journal recovery, and targeted indexing.
pub struct DurableEditService<P> {
    index: IndexService<P>,
    engine: SyntaxEngine<P>,
    authorizer: PathAuthorizer,
    fault_injector: Arc<dyn FaultInjector>,
}

impl<P> DurableEditService<P>
where
    P: GrammarProvider + Clone + Send + Sync,
{
    /// Opens the service and reconciles every incomplete journal row before returning.
    ///
    /// # Errors
    ///
    /// Returns a configuration/store error when roots cannot be authorized or the journal cannot
    /// be listed. Individual recovery conflicts are reported without preventing startup.
    pub fn open(
        index: IndexService<P>,
        provider: P,
        allowed_roots: Vec<PathBuf>,
    ) -> Result<(Self, RecoveryReport), DurableEditError> {
        let authorizer = PathAuthorizer::new(allowed_roots)?;
        let mut service = Self {
            index,
            engine: SyntaxEngine::new(provider),
            authorizer,
            fault_injector: Arc::new(NoFault),
        };
        let recovery = service.recover_incomplete()?;
        Ok((service, recovery))
    }

    #[must_use]
    pub const fn index(&self) -> &IndexService<P> {
        &self.index
    }

    #[must_use]
    pub const fn index_mut(&mut self) -> &mut IndexService<P> {
        &mut self.index
    }

    #[must_use]
    pub fn into_index(self) -> IndexService<P> {
        self.index
    }

    pub fn set_fault_injector(&mut self, injector: Arc<dyn FaultInjector>) {
        self.fault_injector = injector;
    }

    /// Applies one exact structural edit through the durable journal.
    ///
    /// # Errors
    ///
    /// Returns typed stale, authorization, syntax, I/O, journal, index, or recovery failures.
    pub fn edit_node(
        &mut self,
        request: DurableEditRequest,
    ) -> Result<MutationResult, DurableEditError> {
        let project_id = request.locator.scope.file.project_id.clone();
        let relative_path = request.locator.scope.file.relative_path.clone();
        let project = self.project(&project_id)?;
        ensure_generation(request.locator.scope.generation, project.generation)?;
        let authorized = self.authorizer.authorize(
            &project.root_path,
            relative_path.as_str(),
            PathIntent::Update,
        )?;
        let _lease = TargetLease::acquire(authorized.destination())?;
        let source = read_file(authorized.revalidate()?.as_path())?;
        let actual_hash = ContentHash::of(&source);
        if actual_hash != request.locator.scope.file_hash {
            return Err(DurableEditError::StaleSource {
                expected: request.locator.scope.file_hash,
                actual: actual_hash,
            });
        }
        self.ensure_indexed_hash(&project_id, &relative_path, actual_hash)?;
        let snapshot = self.engine.parse(
            request.locator.scope.language_id.clone(),
            Arc::<[u8]>::from(source),
            project.generation,
        )?;
        let next_generation = next_generation(&project_id, project.generation)?;
        let file_context = FileContext::new(project_id.clone(), relative_path.clone());
        let plan = plan_edit(
            &self.engine,
            &snapshot,
            &file_context,
            &request.locator,
            &request.operation,
            next_generation,
            &request.options,
        )?;
        if plan.old_file_hash == plan.new_file_hash {
            return Err(DurableEditError::NoContentChange);
        }
        let before_nodes = self.node_ids(&project_id, &relative_path)?;
        let operation_id = EditOperationId::new(request.operation_id.clone())?;
        let artifacts = ArtifactPaths::new(&operation_id, &authorized)?;
        let journal = NewEditJournalRecord {
            operation_id: operation_id.clone(),
            operation_kind: EditOperationKind::Update,
            project_id: project_id.clone(),
            path: relative_path.clone(),
            original_hash: Some(plan.old_file_hash),
            new_hash: Some(plan.new_file_hash),
            temp_path: Some(artifacts.temp_relative.clone()),
            backup_path: Some(artifacts.backup_relative.clone()),
            created_parent_paths: Vec::new(),
        };
        self.index.store_mut().create_edit_operation(&journal)?;

        let outcome = self.commit_update(&operation_id, &authorized, &artifacts, &plan.source);
        if let Err(error) = &outcome {
            self.record_error(&operation_id, error);
        }
        let refresh = outcome?;
        let after_nodes = self.node_ids(&project_id, &relative_path)?;
        let changed_graph_identities = changed_graph_identities(&before_nodes, &after_nodes);
        Ok(MutationResult {
            operation_id: request.operation_id,
            project_id,
            relative_path,
            old_file_hash: Some(plan.old_file_hash),
            new_file_hash: plan.new_file_hash,
            diff: plan.diff,
            syntax_identities: plan.refreshed_locators,
            changed_graph_identities,
            graph_changes: graph_changes(&before_nodes, &after_nodes),
            generation: refresh.generation,
            diagnostics: plan.diagnostics,
            token_size: plan.token_size,
        })
    }

    /// Creates one authorized project-relative file without overwriting an existing destination.
    ///
    /// # Errors
    ///
    /// Returns typed stale, authorization, parse, I/O, journal, index, or recovery failures.
    pub fn create_file(
        &mut self,
        request: DurableCreateRequest,
    ) -> Result<MutationResult, DurableEditError> {
        let project = self.project(&request.project_id)?;
        ensure_generation(request.expected_generation, project.generation)?;
        let authorized = self.authorizer.authorize(
            &project.root_path,
            request.relative_path.as_str(),
            PathIntent::Create,
        )?;
        let _lease = TargetLease::acquire(authorized.destination())?;
        let next_generation = next_generation(&request.project_id, project.generation)?;
        let validated = validate_create_content(
            &self.engine,
            request.language_id,
            Arc::clone(&request.source),
            next_generation,
            request.parse_policy,
        )?;
        let created_parent_paths = if request.create_parents {
            planned_missing_parents(&authorized)?
        } else {
            authorized.revalidate()?;
            Vec::new()
        };
        let operation_id = EditOperationId::new(request.operation_id.clone())?;
        let artifacts = ArtifactPaths::new(&operation_id, &authorized)?;
        let journal = NewEditJournalRecord {
            operation_id: operation_id.clone(),
            operation_kind: EditOperationKind::Create,
            project_id: request.project_id.clone(),
            path: request.relative_path.clone(),
            original_hash: None,
            new_hash: Some(validated.content_hash),
            temp_path: Some(artifacts.temp_relative.clone()),
            backup_path: None,
            created_parent_paths,
        };
        self.index.store_mut().create_edit_operation(&journal)?;

        let outcome = self.commit_create(
            &operation_id,
            &authorized,
            &artifacts,
            &validated.source,
            request.create_parents,
        );
        if let Err(error) = &outcome {
            self.record_error(&operation_id, error);
        }
        let refresh = outcome?;
        let after_nodes = self.node_ids(&request.project_id, &request.relative_path)?;
        let changed_graph_identities = changed_graph_identities(&BTreeSet::new(), &after_nodes);
        let file_context =
            FileContext::new(request.project_id.clone(), request.relative_path.clone());
        let mut syntax_identities = all_named_locators(&validated.snapshot, &file_context)?;
        syntax_identities.truncate(64);
        let source_len = u64::try_from(validated.source.len())
            .map_err(|_| DurableEditError::GenerationOverflow(request.project_id.clone()))?;
        let diff = SourceDiff {
            old_span: ByteSpan::new(0, 0)?,
            new_span: ByteSpan::new(0, source_len)?,
            removed_hash: ContentHash::of([]),
            inserted_hash: validated.content_hash,
            inserted: Arc::clone(&validated.source),
        };
        Ok(MutationResult {
            operation_id: request.operation_id,
            project_id: request.project_id,
            relative_path: request.relative_path,
            old_file_hash: None,
            new_file_hash: validated.content_hash,
            diff,
            syntax_identities,
            changed_graph_identities,
            graph_changes: graph_changes(&BTreeSet::new(), &after_nodes),
            generation: refresh.generation,
            diagnostics: validated.diagnostics,
            token_size: validated.token_size,
        })
    }
}

#[derive(Debug)]
struct ArtifactPaths {
    temp_relative: ProjectRelativePath,
    backup_relative: ProjectRelativePath,
    temp_absolute: PathBuf,
    backup_absolute: PathBuf,
}

impl ArtifactPaths {
    fn new(
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
    ) -> Result<Self, DurableEditError> {
        let digest = ContentHash::of(operation_id.as_str()).to_string();
        let key = &digest[..20];
        let path = authorized.relative_path().as_str();
        let prefix = path
            .rsplit_once('/')
            .map_or(String::new(), |(parent, _)| format!("{parent}/"));
        let temp_relative = ProjectRelativePath::new(format!("{prefix}.goldeneye-edit-{key}.tmp"))?;
        let backup_relative =
            ProjectRelativePath::new(format!("{prefix}.goldeneye-edit-{key}.bak"))?;
        let temp_absolute = join_relative(authorized.project_root(), &temp_relative);
        let backup_absolute = join_relative(authorized.project_root(), &backup_relative);
        Ok(Self {
            temp_relative,
            backup_relative,
            temp_absolute,
            backup_absolute,
        })
    }
}

#[derive(Debug)]
struct TargetLease {
    key: PathBuf,
}

impl TargetLease {
    fn acquire(path: &Path) -> Result<Self, DurableEditError> {
        let key = target_key(path);
        let targets = ACTIVE_TARGETS.get_or_init(|| Mutex::new(BTreeSet::new()));
        let mut guard = targets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !guard.insert(key.clone()) {
            return Err(DurableEditError::TargetBusy(path.to_path_buf()));
        }
        Ok(Self { key })
    }
}

impl Drop for TargetLease {
    fn drop(&mut self) {
        if let Some(targets) = ACTIVE_TARGETS.get() {
            targets
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&self.key);
        }
    }
}

#[cfg(windows)]
fn target_key(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn target_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn graph_changes(before: &BTreeSet<String>, after: &BTreeSet<String>) -> GraphChanges {
    GraphChanges {
        added: after.difference(before).count(),
        removed: before.difference(after).count(),
        retained: before.intersection(after).count(),
    }
}

fn changed_graph_identities(before: &BTreeSet<String>, after: &BTreeSet<String>) -> Vec<String> {
    let mut identities = before.union(after).cloned().collect::<Vec<_>>();
    identities.truncate(64);
    identities
}

fn ensure_generation(expected: Generation, actual: Generation) -> Result<(), DurableEditError> {
    if expected != actual {
        return Err(DurableEditError::StaleGeneration { expected, actual });
    }
    Ok(())
}

fn next_generation(
    project_id: &ProjectId,
    generation: Generation,
) -> Result<Generation, DurableEditError> {
    generation
        .value()
        .checked_add(1)
        .map(Generation::new)
        .ok_or_else(|| DurableEditError::GenerationOverflow(project_id.clone()))
}

fn required_hash(
    hash: Option<ContentHash>,
    operation_id: &EditOperationId,
    label: &str,
) -> Result<ContentHash, DurableEditError> {
    hash.ok_or_else(|| DurableEditError::RecoveryRequired {
        operation_id: operation_id.as_str().to_owned(),
        reason: format!("journal has no {label} hash"),
    })
}

fn project_relative_from_absolute(
    project_root: &Path,
    path: &Path,
) -> Result<ProjectRelativePath, DurableEditError> {
    let relative = path
        .strip_prefix(project_root)
        .map_err(|source| DurableEditError::Io {
            operation: "making path project-relative",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, source),
        })?;
    let mut segments = Vec::new();
    for component in relative.components() {
        let value = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| DurableEditError::Io {
                operation: "encoding project-relative path",
                path: path.to_path_buf(),
                source: io::Error::new(io::ErrorKind::InvalidData, "path is not valid UTF-8"),
            })?;
        segments.push(value);
    }
    Ok(ProjectRelativePath::new(segments.join("/"))?)
}

fn planned_missing_parents(
    authorized: &AuthorizedPath,
) -> Result<Vec<ProjectRelativePath>, DurableEditError> {
    let segments = authorized.relative_path().as_str().split('/');
    let parent_count = segments.clone().count().saturating_sub(1);
    let mut current = authorized.project_root().to_path_buf();
    let mut missing = Vec::new();
    for segment in segments.take(parent_count) {
        current.push(segment);
        if !path_present(&current)? {
            missing.push(project_relative_from_absolute(
                authorized.project_root(),
                &current,
            )?);
        }
    }
    Ok(missing)
}

fn join_relative(root: &Path, path: &ProjectRelativePath) -> PathBuf {
    path.as_str()
        .split('/')
        .fold(root.to_path_buf(), |current, segment| current.join(segment))
}

fn metadata(path: &Path) -> Result<fs::Metadata, DurableEditError> {
    fs::metadata(path).map_err(|source| DurableEditError::Io {
        operation: "reading metadata for",
        path: path.to_path_buf(),
        source,
    })
}

fn read_file(path: &Path) -> Result<Vec<u8>, DurableEditError> {
    let mut file = File::open(path).map_err(|source| DurableEditError::Io {
        operation: "opening",
        path: path.to_path_buf(),
        source,
    })?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| DurableEditError::Io {
            operation: "reading",
            path: path.to_path_buf(),
            source,
        })?;
    Ok(bytes)
}

fn write_temp(
    path: &Path,
    bytes: &[u8],
    permissions: Option<Permissions>,
) -> Result<(), DurableEditError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| DurableEditError::Io {
            operation: "creating temporary file",
            path: path.to_path_buf(),
            source,
        })?;
    if let Some(permissions) = permissions {
        file.set_permissions(permissions)
            .map_err(|source| DurableEditError::Io {
                operation: "copying permissions to",
                path: path.to_path_buf(),
                source,
            })?;
    }
    file.write_all(bytes)
        .map_err(|source| DurableEditError::Io {
            operation: "writing temporary file",
            path: path.to_path_buf(),
            source,
        })?;
    file.flush().map_err(|source| DurableEditError::Io {
        operation: "flushing temporary file",
        path: path.to_path_buf(),
        source,
    })?;
    file.sync_all().map_err(|source| DurableEditError::Io {
        operation: "synchronizing temporary file",
        path: path.to_path_buf(),
        source,
    })
}

fn ensure_file_hash(path: &Path, expected: ContentHash) -> Result<(), DurableEditError> {
    let actual = ContentHash::of(read_file(path)?);
    if actual != expected {
        return Err(DurableEditError::StaleSource { expected, actual });
    }
    Ok(())
}

fn hash_if_file(path: &Path) -> Result<Option<ContentHash>, DurableEditError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() || metadata.file_type().is_symlink() => {
            Ok(Some(ContentHash::of(read_file(path)?)))
        }
        Ok(_) => Err(DurableEditError::Io {
            operation: "hashing non-file",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "path is not a regular file"),
        }),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(DurableEditError::Io {
            operation: "inspecting",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn path_present(path: &Path) -> Result<bool, DurableEditError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(DurableEditError::Io {
            operation: "inspecting",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn rename_new(source_path: &Path, destination: &Path) -> Result<(), DurableEditError> {
    fs::hard_link(source_path, destination).map_err(|source| DurableEditError::Io {
        operation: "atomically linking recovery file to",
        path: destination.to_path_buf(),
        source,
    })?;
    fs::remove_file(source_path).map_err(|source| DurableEditError::Io {
        operation: "removing prior recovery link",
        path: source_path.to_path_buf(),
        source,
    })
}

fn hard_link_new(source_path: &Path, destination: &Path) -> Result<(), DurableEditError> {
    fs::hard_link(source_path, destination).map_err(|source| DurableEditError::Io {
        operation: "creating no-overwrite destination",
        path: destination.to_path_buf(),
        source,
    })
}

fn remove_if_exists(path: &Path) -> Result<(), DurableEditError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(DurableEditError::Io {
            operation: "removing recovery file",
            path: path.to_path_buf(),
            source,
        }),
    }
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> Result<(), DurableEditError> {
    let parent = path.parent().ok_or_else(|| DurableEditError::Io {
        operation: "locating parent for",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| DurableEditError::Io {
            operation: "synchronizing parent of",
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(windows)]
fn sync_parent(path: &Path) -> Result<(), DurableEditError> {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    let parent = path.parent().ok_or_else(|| DurableEditError::Io {
        operation: "locating parent for",
        path: path.to_path_buf(),
        source: io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"),
    })?;
    let result = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(parent)
        .and_then(|directory| directory.sync_all());
    match result {
        Ok(()) => Ok(()),
        // Windows FlushFileBuffers rejects directory handles. File payloads and SQLite remain
        // synchronously flushed; rename-directory durability relies on NTFS journaling here.
        Err(source)
            if matches!(
                source.kind(),
                io::ErrorKind::PermissionDenied | io::ErrorKind::InvalidInput
            ) =>
        {
            Ok(())
        }
        Err(source) => Err(DurableEditError::Io {
            operation: "synchronizing parent of",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn cleanup_artifacts(artifacts: &ArtifactPaths) -> Result<(), DurableEditError> {
    remove_if_exists(&artifacts.temp_absolute)?;
    remove_if_exists(&artifacts.backup_absolute)
}

fn validate_journal_artifacts(
    record: &EditJournalRecord,
    artifacts: &ArtifactPaths,
) -> Result<(), DurableEditError> {
    let temp_matches = record.temp_path.as_ref() == Some(&artifacts.temp_relative);
    let backup_matches = match record.operation_kind {
        EditOperationKind::Create => record.backup_path.is_none(),
        EditOperationKind::Update | EditOperationKind::Delete => {
            record.backup_path.as_ref() == Some(&artifacts.backup_relative)
        }
    };
    if !temp_matches || !backup_matches {
        return Err(DurableEditError::JournalPathMismatch(
            record.operation_id.as_str().to_owned(),
        ));
    }
    Ok(())
}

fn remove_empty_confined_directory(
    project_root: &Path,
    path: &Path,
) -> Result<(), DurableEditError> {
    if !path_present(path)? || path == project_root {
        return Ok(());
    }
    let canonical = fs::canonicalize(path).map_err(|source| DurableEditError::Io {
        operation: "canonicalizing recovery directory",
        path: path.to_path_buf(),
        source,
    })?;
    if !canonical.starts_with(project_root) {
        return Err(DurableEditError::Io {
            operation: "validating recovery directory",
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "directory escaped project"),
        });
    }
    let mut entries = fs::read_dir(&canonical).map_err(|source| DurableEditError::Io {
        operation: "reading recovery directory",
        path: canonical.clone(),
        source,
    })?;
    if entries
        .next()
        .transpose()
        .map_err(|source| DurableEditError::Io {
            operation: "reading recovery directory",
            path: canonical.clone(),
            source,
        })?
        .is_some()
    {
        return Ok(());
    }
    fs::remove_dir(&canonical).map_err(|source| DurableEditError::Io {
        operation: "removing empty recovery directory",
        path: canonical,
        source,
    })
}

impl<P> DurableEditService<P>
where
    P: GrammarProvider + Clone + Send + Sync,
{
    fn project(
        &self,
        project_id: &ProjectId,
    ) -> Result<goldeneye_domain::ProjectRecord, DurableEditError> {
        self.index
            .store()
            .get_project(project_id)?
            .ok_or_else(|| DurableEditError::ProjectNotFound(project_id.clone()))
    }

    fn ensure_indexed_hash(
        &self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
        actual_hash: ContentHash,
    ) -> Result<(), DurableEditError> {
        let file_id = FileId::new(project_id.clone(), path.clone());
        let indexed = self
            .index
            .store()
            .get_file(&file_id)?
            .ok_or_else(|| DurableEditError::FileNotIndexed(path.clone()))?;
        if indexed.content_hash != actual_hash {
            return Err(DurableEditError::StaleSource {
                expected: indexed.content_hash,
                actual: actual_hash,
            });
        }
        Ok(())
    }

    fn node_ids(
        &self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<BTreeSet<String>, DurableEditError> {
        let file_id = FileId::new(project_id.clone(), path.clone());
        Ok(self
            .index
            .store()
            .nodes_for_file(&file_id)?
            .into_iter()
            .map(|node| node.id.as_str().to_owned())
            .collect())
    }

    fn commit_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        source: &[u8],
    ) -> Result<FileRefreshResult, DurableEditError> {
        self.check_fault(operation_id, FaultPoint::AfterJournal)?;
        self.check_fault(operation_id, FaultPoint::BeforeWrite)?;
        let permissions = metadata(authorized.destination())?.permissions();
        write_temp(&artifacts.temp_absolute, source, Some(permissions))?;
        self.check_fault(operation_id, FaultPoint::AfterTemp)?;

        let record = self.operation(operation_id)?;
        let expected_old = required_hash(record.original_hash, operation_id, "original")?;
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        authorized.revalidate()?;
        ensure_file_hash(authorized.destination(), expected_old)?;
        ensure_file_hash(&artifacts.temp_absolute, expected_new)?;
        rename_new(authorized.destination(), &artifacts.backup_absolute)?;
        sync_parent(authorized.destination())?;
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::Prepared,
            EditPhase::BackupReady,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterBackup)?;

        rename_new(&artifacts.temp_absolute, authorized.destination())?;
        sync_parent(authorized.destination())?;
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::BackupReady,
            EditPhase::Replaced,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterRename)?;
        self.check_fault(operation_id, FaultPoint::DuringReindex)?;

        let refresh = match self.refresh_existing(&record.project_id, &record.path) {
            Ok(refresh) => refresh,
            Err(refresh_error) => {
                let reason = refresh_error.to_string();
                if let Err(rollback) = self.rollback_update(operation_id, authorized, artifacts) {
                    return Err(DurableEditError::RecoveryRequired {
                        operation_id: operation_id.as_str().to_owned(),
                        reason: format!(
                            "index refresh failed ({reason}); rollback failed ({rollback})"
                        ),
                    });
                }
                return Err(DurableEditError::RefreshRejected { reason });
            }
        };
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::Replaced,
            EditPhase::Indexed,
        )?;
        self.check_fault(operation_id, FaultPoint::Cleanup)?;
        remove_if_exists(&artifacts.backup_absolute)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::Indexed,
            EditPhase::Committed,
        )?;
        Ok(refresh)
    }

    fn commit_create(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        source: &[u8],
        create_parents: bool,
    ) -> Result<FileRefreshResult, DurableEditError> {
        self.check_fault(operation_id, FaultPoint::AfterJournal)?;
        self.check_fault(operation_id, FaultPoint::BeforeWrite)?;
        let created = create_parents
            .then(|| authorized.create_parent_directories())
            .transpose()?;
        write_temp(&artifacts.temp_absolute, source, None)?;
        self.check_fault(operation_id, FaultPoint::AfterTemp)?;
        let record = self.operation(operation_id)?;
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        ensure_file_hash(&artifacts.temp_absolute, expected_new)?;
        authorized.revalidate()?;
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::Prepared,
            EditPhase::BackupReady,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterBackup)?;

        hard_link_new(&artifacts.temp_absolute, authorized.destination())?;
        remove_if_exists(&artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::BackupReady,
            EditPhase::Replaced,
        )?;
        self.check_fault(operation_id, FaultPoint::AfterRename)?;
        self.check_fault(operation_id, FaultPoint::DuringReindex)?;

        let refresh = match self.refresh_existing(&record.project_id, &record.path) {
            Ok(refresh) => refresh,
            Err(refresh_error) => {
                let reason = refresh_error.to_string();
                if let Err(rollback) =
                    self.rollback_create(operation_id, authorized, artifacts, created.as_ref())
                {
                    return Err(DurableEditError::RecoveryRequired {
                        operation_id: operation_id.as_str().to_owned(),
                        reason: format!(
                            "index refresh failed ({reason}); rollback failed ({rollback})"
                        ),
                    });
                }
                return Err(DurableEditError::RefreshRejected { reason });
            }
        };
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::Replaced,
            EditPhase::Indexed,
        )?;
        self.check_fault(operation_id, FaultPoint::Cleanup)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.index.store_mut().transition_edit_operation(
            operation_id,
            EditPhase::Indexed,
            EditPhase::Committed,
        )?;
        Ok(refresh)
    }

    fn refresh_existing(
        &mut self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<FileRefreshResult, DurableEditError> {
        let refresh = self.index.refresh_file(project_id, path)?;
        if refresh.status == FileRefreshStatus::RejectedSyntax {
            return Err(DurableEditError::RefreshRejected {
                reason: format!(
                    "parser returned {} diagnostic groups",
                    refresh.diagnostics.len()
                ),
            });
        }
        if !matches!(
            refresh.status,
            FileRefreshStatus::Updated | FileRefreshStatus::Unchanged
        ) {
            return Err(DurableEditError::RefreshRejected {
                reason: format!("unexpected refresh status {:?}", refresh.status),
            });
        }
        Ok(refresh)
    }

    fn refresh_absent(
        &mut self,
        project_id: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<FileRefreshResult, DurableEditError> {
        let refresh = self.index.refresh_file(project_id, path)?;
        if !matches!(
            refresh.status,
            FileRefreshStatus::Deleted | FileRefreshStatus::Unchanged
        ) {
            return Err(DurableEditError::RefreshRejected {
                reason: format!("unexpected absent-file refresh status {:?}", refresh.status),
            });
        }
        Ok(refresh)
    }

    fn rollback_update(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
    ) -> Result<(), DurableEditError> {
        let record = self.operation(operation_id)?;
        let expected_old = required_hash(record.original_hash, operation_id, "original")?;
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        ensure_file_hash(authorized.destination(), expected_new)?;
        ensure_file_hash(&artifacts.backup_absolute, expected_old)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        rename_new(authorized.destination(), &artifacts.temp_absolute)?;
        rename_new(&artifacts.backup_absolute, authorized.destination())?;
        sync_parent(authorized.destination())?;
        self.refresh_existing(&record.project_id, &record.path)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        self.index.store_mut().transition_edit_operation(
            operation_id,
            record.phase,
            EditPhase::RolledBack,
        )?;
        Ok(())
    }

    fn rollback_create(
        &mut self,
        operation_id: &EditOperationId,
        authorized: &AuthorizedPath,
        artifacts: &ArtifactPaths,
        created: Option<&crate::path_auth::CreatedDirectories>,
    ) -> Result<(), DurableEditError> {
        let record = self.operation(operation_id)?;
        let expected_new = required_hash(record.new_hash, operation_id, "new")?;
        ensure_file_hash(authorized.destination(), expected_new)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        rename_new(authorized.destination(), &artifacts.temp_absolute)?;
        sync_parent(authorized.destination())?;
        self.refresh_absent(&record.project_id, &record.path)?;
        remove_if_exists(&artifacts.temp_absolute)?;
        if let Some(created) = created {
            created.rollback_empty()?;
        }
        self.index.store_mut().transition_edit_operation(
            operation_id,
            record.phase,
            EditPhase::RolledBack,
        )?;
        Ok(())
    }

    fn operation(
        &self,
        operation_id: &EditOperationId,
    ) -> Result<EditJournalRecord, DurableEditError> {
        self.index
            .store()
            .get_edit_operation(operation_id)?
            .ok_or_else(|| StoreError::EditOperationNotFound(operation_id.clone()).into())
    }

    fn check_fault(
        &mut self,
        operation_id: &EditOperationId,
        point: FaultPoint,
    ) -> Result<(), DurableEditError> {
        if let Err(message) = self.fault_injector.check(point) {
            let error = DurableEditError::InjectedFault { point, message };
            self.record_error(operation_id, &error);
            return Err(error);
        }
        Ok(())
    }

    fn record_error(&mut self, operation_id: &EditOperationId, error: &DurableEditError) {
        let message = error.to_string();
        let compact = message.chars().take(1024).collect::<String>();
        let _ = self
            .index
            .store_mut()
            .set_edit_operation_error(operation_id, Some(&compact));
    }
}

impl<P> DurableEditService<P>
where
    P: GrammarProvider + Clone + Send + Sync,
{
    /// Reconciles every nonterminal journal row against authoritative on-disk hashes.
    ///
    /// # Errors
    ///
    /// Returns a store error only when the incomplete journal cannot be listed. Per-operation
    /// conflicts remain journaled with recovery material and are returned as unresolved entries.
    pub fn recover_incomplete(&mut self) -> Result<RecoveryReport, DurableEditError> {
        let records = self.index.store().list_incomplete_edit_operations()?;
        let mut entries = Vec::with_capacity(records.len());
        for record in records {
            let operation_id = record.operation_id.as_str().to_owned();
            let project_id = record.project_id.clone();
            let relative_path = record.path.clone();
            match self.recover_one(&record) {
                Ok(action) => entries.push(RecoveryEntry {
                    operation_id,
                    project_id,
                    relative_path,
                    resolved: true,
                    action,
                    error: None,
                }),
                Err(error) => {
                    self.record_error(&record.operation_id, &error);
                    entries.push(RecoveryEntry {
                        operation_id,
                        project_id,
                        relative_path,
                        resolved: false,
                        action: RecoveryAction::PreservedConflict,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        Ok(RecoveryReport { entries })
    }

    fn recover_one(
        &mut self,
        record: &EditJournalRecord,
    ) -> Result<RecoveryAction, DurableEditError> {
        let project = self.project(&record.project_id)?;
        let lexical = join_relative(Path::new(&project.root_path), &record.path);
        let intent = if path_present(&lexical)? {
            PathIntent::Update
        } else {
            PathIntent::Create
        };
        let authorized =
            self.authorizer
                .authorize(&project.root_path, record.path.as_str(), intent)?;
        let _lease = TargetLease::acquire(authorized.destination())?;
        let artifacts = ArtifactPaths::new(&record.operation_id, &authorized)?;
        validate_journal_artifacts(record, &artifacts)?;

        let actual = hash_if_file(authorized.destination())?;
        if actual.is_some() && actual == record.new_hash {
            self.refresh_existing(&record.project_id, &record.path)?;
            let indexed = self.advance_to_indexed(record)?;
            cleanup_artifacts(&artifacts)?;
            sync_parent(authorized.destination())?;
            self.index.store_mut().transition_edit_operation(
                &record.operation_id,
                indexed.phase,
                EditPhase::Committed,
            )?;
            return Ok(RecoveryAction::CommittedNewSource);
        }

        if actual.is_some() && actual == record.original_hash {
            self.refresh_existing(&record.project_id, &record.path)?;
            cleanup_artifacts(&artifacts)?;
            Self::rollback_recorded_parents(authorized.project_root(), record)?;
            self.mark_rolled_back(record)?;
            return Ok(RecoveryAction::RestoredOriginalSource);
        }

        if actual.is_none() {
            if record.original_hash.is_none() {
                self.refresh_absent(&record.project_id, &record.path)?;
                cleanup_artifacts(&artifacts)?;
                Self::rollback_recorded_parents(authorized.project_root(), record)?;
                self.mark_rolled_back(record)?;
                return Ok(RecoveryAction::RemovedIncompleteCreate);
            }
            let expected_old =
                required_hash(record.original_hash, &record.operation_id, "original")?;
            if hash_if_file(&artifacts.backup_absolute)? != Some(expected_old) {
                return Err(DurableEditError::RecoveryRequired {
                    operation_id: record.operation_id.as_str().to_owned(),
                    reason: "target is missing and backup does not match original hash".to_owned(),
                });
            }
            rename_new(&artifacts.backup_absolute, authorized.destination())?;
            sync_parent(authorized.destination())?;
            self.refresh_existing(&record.project_id, &record.path)?;
            remove_if_exists(&artifacts.temp_absolute)?;
            self.mark_rolled_back(record)?;
            return Ok(RecoveryAction::RestoredOriginalSource);
        }

        // An external writer won the race. Make the graph reflect that source, but retain the
        // journal and both known versions for a human/agent conflict decision.
        self.refresh_existing(&record.project_id, &record.path)?;
        Err(DurableEditError::RecoveryRequired {
            operation_id: record.operation_id.as_str().to_owned(),
            reason: format!(
                "actual target hash {} matches neither journal version",
                actual.expect("actual hash is present")
            ),
        })
    }

    fn advance_to_indexed(
        &mut self,
        initial: &EditJournalRecord,
    ) -> Result<EditJournalRecord, DurableEditError> {
        let mut current = self.operation(&initial.operation_id)?;
        while current.phase != EditPhase::Indexed {
            let next = match current.phase {
                EditPhase::Prepared => EditPhase::BackupReady,
                EditPhase::BackupReady => EditPhase::Replaced,
                EditPhase::Replaced => EditPhase::Indexed,
                EditPhase::Indexed => break,
                EditPhase::Committed | EditPhase::RolledBack => {
                    return Err(DurableEditError::RecoveryRequired {
                        operation_id: current.operation_id.as_str().to_owned(),
                        reason: format!("unexpected terminal phase {:?}", current.phase),
                    });
                }
            };
            current = self.index.store_mut().transition_edit_operation(
                &current.operation_id,
                current.phase,
                next,
            )?;
        }
        Ok(current)
    }

    fn mark_rolled_back(&mut self, initial: &EditJournalRecord) -> Result<(), DurableEditError> {
        let current = self.operation(&initial.operation_id)?;
        if current.phase != EditPhase::RolledBack {
            self.index.store_mut().transition_edit_operation(
                &current.operation_id,
                current.phase,
                EditPhase::RolledBack,
            )?;
        }
        Ok(())
    }

    fn rollback_recorded_parents(
        project_root: &Path,
        record: &EditJournalRecord,
    ) -> Result<(), DurableEditError> {
        for relative in record.created_parent_paths.iter().rev() {
            let path = join_relative(project_root, relative);
            remove_empty_confined_directory(project_root, &path)?;
        }
        Ok(())
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
