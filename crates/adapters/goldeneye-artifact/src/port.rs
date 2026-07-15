use std::path::Path;

use goldeneye_ports::{ArtifactPersistence, PortError};

use crate::{ArtifactQuality, artifact_exists, export_artifact, import_artifact};

/// Filesystem-backed shared artifact persistence.
#[derive(Debug, Clone, Copy, Default)]
pub struct FileArtifactPersistence;

impl ArtifactPersistence for FileArtifactPersistence {
    fn exists(&self, repository: &Path) -> bool {
        artifact_exists(repository)
    }

    fn import(&self, repository: &Path, database: &Path) -> Result<(), PortError> {
        import_artifact(repository, database)
            .map(|_| ())
            .map_err(PortError::new)
    }

    fn export(&self, database: &Path, repository: &Path, project: &str) -> Result<(), PortError> {
        export_artifact(database, repository, project, ArtifactQuality::Best)
            .map(|_| ())
            .map_err(PortError::new)
    }
}
