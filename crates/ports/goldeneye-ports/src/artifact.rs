use std::path::Path;

use crate::PortError;

/// Persists and restores shared repository graph artifacts.
pub trait ArtifactPersistence: Send + Sync {
    /// Reports whether a complete artifact is available for `repository`.
    fn exists(&self, repository: &Path) -> bool;

    /// Restores a verified artifact into `database`.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when the artifact cannot be verified or installed.
    fn import(&self, repository: &Path, database: &Path) -> Result<(), PortError>;

    /// Exports the indexed project using the application-selected durable quality.
    ///
    /// # Errors
    ///
    /// Returns an adapter error when a consistent artifact cannot be written.
    fn export(&self, database: &Path, repository: &Path, project: &str) -> Result<(), PortError>;
}
