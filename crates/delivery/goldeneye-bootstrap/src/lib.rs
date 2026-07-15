#![forbid(unsafe_code)]

//! Production composition for Goldeneye services and background indexing.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_git::GitCommandRepository;
use goldeneye_services::{
    IndexRepositoryMode, IndexRepositoryRequest, ProjectId, ServiceConfig, ServiceDependencies,
    ServiceError, Services,
};
use goldeneye_store::SqliteRepositoryFactory;
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
use goldeneye_watcher::{IndexDisposition, Indexer, WatchRuntime, Watcher, WatcherConfig};

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

/// Owns one shared application service graph and its single background watcher runtime.
///
/// Dropping this value signals the watcher to stop, wakes its thread, and joins it. Drop may
/// block until an active poll or index operation completes because those operations are
/// intentionally synchronous and are not forcefully cancelled.
pub struct BootstrapRuntime {
    services: Services,
    watcher: Arc<Watcher<ServiceIndexer>>,
    watch_runtime: Option<WatchRuntime>,
}

impl BootstrapRuntime {
    /// Creates, seeds, and starts one best-effort watcher over `services`.
    #[must_use]
    pub fn new(services: Services) -> Self {
        let watcher = Arc::new(Watcher::new(
            WatcherConfig::default(),
            ServiceIndexer::new(services.clone()),
        ));
        if let Ok(projects) = services.list_projects() {
            for project in projects {
                let _ = watcher.watch(project.project, project.root_path);
            }
        }
        let watch_runtime = watcher.spawn().ok();
        Self {
            services,
            watcher,
            watch_runtime,
        }
    }

    /// Creates one runtime from explicit service configuration.
    #[must_use]
    pub fn from_config(config: ServiceConfig) -> Self {
        Self::new(Services::new(config, service_dependencies()))
    }

    /// Creates one runtime from process environment configuration.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration or recovery error when services cannot be opened.
    pub fn from_env() -> Result<Self, ServiceError> {
        Services::from_env(service_dependencies()).map(Self::new)
    }

    #[must_use]
    pub const fn services(&self) -> &Services {
        &self.services
    }

    #[must_use]
    pub const fn watcher(&self) -> &Arc<Watcher<ServiceIndexer>> {
        &self.watcher
    }
}

impl Drop for BootstrapRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.watch_runtime.take() {
            runtime.stop();
        }
    }
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
