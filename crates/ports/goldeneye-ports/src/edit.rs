use std::error::Error;
use std::fmt;

use goldeneye_domain::{
    ContentHash, FileId, FileRecord, Generation, GraphNode, ProjectId, ProjectRecord,
    ProjectRelativePath,
};

use crate::PortError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditOperationId(String);

impl EditOperationId {
    /// Creates a stable edit-operation identifier.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty or NUL-containing value.
    pub fn new(value: impl Into<String>) -> Result<Self, PortError> {
        let value = value.into();
        if value.is_empty() || value.contains('\0') {
            return Err(PortError::new(InvalidEditOperationId));
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug)]
struct InvalidEditOperationId;

impl fmt::Display for InvalidEditOperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("edit operation ID must be non-empty and contain no NUL bytes")
    }
}

impl Error for InvalidEditOperationId {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOperationKind {
    Create,
    Update,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPhase {
    Prepared,
    BackupReady,
    Replaced,
    Indexed,
    Committed,
    RolledBack,
}

impl EditPhase {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Committed | Self::RolledBack)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEditJournalRecord {
    pub operation_id: EditOperationId,
    pub operation_kind: EditOperationKind,
    pub project_id: ProjectId,
    pub path: ProjectRelativePath,
    pub original_hash: Option<ContentHash>,
    pub new_hash: Option<ContentHash>,
    pub temp_path: Option<ProjectRelativePath>,
    pub backup_path: Option<ProjectRelativePath>,
    pub created_parent_paths: Vec<ProjectRelativePath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditJournalRecord {
    pub operation_id: EditOperationId,
    pub record_version: u32,
    pub operation_kind: EditOperationKind,
    pub project_id: ProjectId,
    pub path: ProjectRelativePath,
    pub original_hash: Option<ContentHash>,
    pub new_hash: Option<ContentHash>,
    pub temp_path: Option<ProjectRelativePath>,
    pub backup_path: Option<ProjectRelativePath>,
    pub created_parent_paths: Vec<ProjectRelativePath>,
    pub phase: EditPhase,
    pub created_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
}

/// Durable journal operations required by edit use cases.
pub trait EditRepository: Send {
    /// Finds one indexed project.
    ///
    /// # Errors
    ///
    /// Returns an error when the project registry cannot be read.
    fn get_project(&self, project: &ProjectId) -> Result<Option<ProjectRecord>, PortError>;

    /// Finds one indexed file.
    ///
    /// # Errors
    ///
    /// Returns an error when the file record cannot be read.
    fn get_file(&self, file: &FileId) -> Result<Option<FileRecord>, PortError>;

    /// Lists syntax graph nodes originating from one file.
    ///
    /// # Errors
    ///
    /// Returns an error when graph nodes cannot be read.
    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError>;

    /// Creates one prepared journal record.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails.
    fn create_edit_operation(
        &mut self,
        record: &NewEditJournalRecord,
    ) -> Result<EditJournalRecord, PortError>;

    /// Advances one journal record through an expected phase transition.
    ///
    /// # Errors
    ///
    /// Returns an error for missing records, stale phases, invalid transitions, or persistence
    /// failures.
    fn transition_edit_operation(
        &mut self,
        operation_id: &EditOperationId,
        expected: EditPhase,
        next: EditPhase,
    ) -> Result<EditJournalRecord, PortError>;

    /// Finds one journal record.
    ///
    /// # Errors
    ///
    /// Returns an error when the record cannot be read or decoded.
    fn get_edit_operation(
        &self,
        operation_id: &EditOperationId,
    ) -> Result<Option<EditJournalRecord>, PortError>;

    /// Lists nonterminal journal records in stable creation order.
    ///
    /// # Errors
    ///
    /// Returns an error when records cannot be read or decoded.
    fn list_incomplete_edit_operations(&self) -> Result<Vec<EditJournalRecord>, PortError>;

    /// Replaces one journal record's recovery error.
    ///
    /// # Errors
    ///
    /// Returns an error when the record is missing or cannot be persisted.
    fn set_edit_operation_error(
        &mut self,
        operation_id: &EditOperationId,
        error: Option<&str>,
    ) -> Result<EditJournalRecord, PortError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditRefreshStatus {
    Updated,
    Deleted,
    Unchanged,
    RejectedSyntax,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRefreshResult {
    pub status: EditRefreshStatus,
    pub generation: Generation,
    pub diagnostics: usize,
}

/// Targeted indexing required after durable source edits.
pub trait EditIndexer: Send {
    /// Refreshes one project-relative file in the durable graph.
    ///
    /// # Errors
    ///
    /// Returns an error when discovery, parsing, graph assembly, or persistence fails.
    fn refresh_file(
        &mut self,
        project: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<EditRefreshResult, PortError>;
}
