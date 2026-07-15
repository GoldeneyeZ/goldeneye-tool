use std::path::Path;

use crate::{
    AdrTraceRepository, CrossLinkRepository, EditRepository, IndexRepository, PortError,
    ProjectAdministrationRepository, QueryRepository,
};

/// Creates application repository ports without exposing adapter-owned stores.
pub trait RepositoryFactory: Send + Sync {
    /// Creates or migrates the durable repository at `path`.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the repository cannot be initialized.
    fn initialize(&self, path: &Path) -> Result<(), PortError>;

    /// Opens an existing repository for read-only query use.
    ///
    /// # Errors
    ///
    /// Returns an adapter error without creating or migrating a missing repository.
    fn open_query(&self, path: &Path) -> Result<Box<dyn QueryRepository>, PortError>;

    /// Opens a writable repository for indexing.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the repository cannot be opened or migrated.
    fn open_index(&self, path: &Path) -> Result<Box<dyn IndexRepository>, PortError>;

    /// Opens a writable repository for durable edit journaling.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the repository cannot be opened or migrated.
    fn open_edit(&self, path: &Path) -> Result<Box<dyn EditRepository>, PortError>;

    /// Opens a writable repository for cross-project edge rebuilding.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the repository cannot be opened or migrated.
    fn open_crosslink(&self, path: &Path) -> Result<Box<dyn CrossLinkRepository>, PortError>;

    /// Opens a writable repository for project administration.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the repository cannot be opened or migrated.
    fn open_project_administration(
        &self,
        path: &Path,
    ) -> Result<Box<dyn ProjectAdministrationRepository>, PortError>;

    /// Opens a writable repository for ADR and runtime-trace persistence.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the repository cannot be opened or migrated.
    fn open_adr_traces(&self, path: &Path) -> Result<Box<dyn AdrTraceRepository>, PortError>;
}
