#![forbid(unsafe_code)]

//! Production composition for Goldeneye services and background indexing.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_git::GitCommandRepository;
use goldeneye_services::{
    IndexRepositoryMode, IndexRepositoryRequest, ProjectId, ServiceDependencies, Services,
};
use goldeneye_store::SqliteRepositoryFactory;
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
use goldeneye_watcher::{IndexDisposition, Indexer};

/// Builds the production adapter set used by Goldeneye delivery crates.
#[must_use]
pub fn service_dependencies() -> ServiceDependencies {
    let discovery = Arc::new(FileSystemDiscovery);
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
        discovery,
        Arc::new(SqliteRepositoryFactory),
        Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
        Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
    )
}

/// Adapts shared application services to the generic background watcher.
pub struct ServiceIndexer {
    services: Services,
    busy: AtomicBool,
}

impl ServiceIndexer {
    #[must_use]
    pub const fn new(services: Services) -> Self {
        Self {
            services,
            busy: AtomicBool::new(false),
        }
    }
}

impl Indexer for ServiceIndexer {
    fn index(&self, project: &str, root: &Path) -> Result<IndexDisposition, String> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(IndexDisposition::Busy);
        }
        let result = self.services.index_repository(&IndexRepositoryRequest {
            repo_path: root.to_owned(),
            name: Some(project.to_owned()),
            mode: IndexRepositoryMode::Fast,
            persistence: false,
        });
        self.busy.store(false, Ordering::Release);
        result.map_err(|error| error.to_string())?;
        Ok(IndexDisposition::Indexed)
    }

    fn prune(&self, project: &str, _root: &Path) -> Result<(), String> {
        if !self.services.config().database_path().is_file() {
            return Ok(());
        }
        let project = ProjectId::new(project).map_err(|error| error.to_string())?;
        self.services
            .delete_project(&project)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}
