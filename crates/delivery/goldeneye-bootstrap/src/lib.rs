#![forbid(unsafe_code)]

//! Production composition for Goldeneye services and background indexing.

use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_domain::LanguageId;
use goldeneye_git::GitCommandRepository;
use goldeneye_ports::{
    LanguageClassifier, PortError, RepositoryDiscovery, RepositoryDiscoveryOptions,
    RepositoryDiscoveryReport, SourceDiscovery,
};
use goldeneye_services::{
    IndexRepositoryMode, IndexRepositoryRequest, ProjectId, ServiceConfig, ServiceDependencies,
    ServiceError, Services,
};
use goldeneye_store::{SqliteRepositoryFactory, Store, StoreError};
#[cfg(feature = "full-grammar-pack")]
use goldeneye_syntax::CoreFirstGrammarProvider;
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
use goldeneye_watcher::{IndexDisposition, Indexer, WatchRuntime, Watcher, WatcherConfig};

/// Controls background repository watching for delivery runtimes.
///
/// `0`, `false`, `off`, or `disabled` selects the read-only, watcher-free
/// runtime. Every other value and an unset variable preserve watching.
pub const WATCHER_ENABLED_ENV: &str = "GOLDENEYE_WATCHER_ENABLED";

/// Builds the production adapter set used by Goldeneye delivery crates.
#[must_use]
pub fn service_dependencies() -> ServiceDependencies {
    let pack = env::var("GOLDENEYE_GRAMMAR_PACK").ok();
    service_dependencies_for_pack(configured_grammar_pack(pack.as_deref()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GrammarPack {
    Core,
    Full,
}

fn configured_grammar_pack(value: Option<&str>) -> GrammarPack {
    match value {
        Some(value) if value.eq_ignore_ascii_case("full") => GrammarPack::Full,
        _ => GrammarPack::Core,
    }
}

fn configured_include_paths() -> Vec<PathBuf> {
    env::var("GOLDENEYE_INCLUDE_PATHS")
        .ok()
        .map(|value| {
            value
                .split(';')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

fn watcher_enabled_from_value(value: Option<std::ffi::OsString>) -> bool {
    !value.is_some_and(|value| {
        matches!(
            value.to_string_lossy().trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "disabled"
        )
    })
}

fn path_is_included(path: &Path, includes: &[PathBuf]) -> bool {
    includes.iter().any(|include| path.starts_with(include))
}

struct FilteredSourceDiscovery {
    includes: Vec<PathBuf>,
}

impl LanguageClassifier for FilteredSourceDiscovery {
    fn classify(&self, path: &Path) -> Option<LanguageId> {
        FileSystemDiscovery.classify(path)
    }
}

impl RepositoryDiscovery for FilteredSourceDiscovery {
    fn discover(
        &self,
        root: &Path,
        options: &RepositoryDiscoveryOptions,
    ) -> Result<RepositoryDiscoveryReport, PortError> {
        let mut report = FileSystemDiscovery.discover(root, options)?;
        report
            .files
            .retain(|file| path_is_included(&file.relative_path, &self.includes));
        Ok(report)
    }
}

fn service_dependencies_for_pack(pack: GrammarPack) -> ServiceDependencies {
    let includes = configured_include_paths();
    let discovery: Arc<dyn SourceDiscovery> = if includes.is_empty() {
        Arc::new(FileSystemDiscovery)
    } else {
        Arc::new(FilteredSourceDiscovery { includes })
    };
    match pack {
        GrammarPack::Core => ServiceDependencies::new(
            Arc::new(FileArtifactPersistence),
            Arc::new(GitCommandRepository),
            discovery,
            Arc::new(SqliteRepositoryFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
        ),
        #[cfg(not(feature = "full-grammar-pack"))]
        GrammarPack::Full => {
            panic!(
                "GOLDENEYE_GRAMMAR_PACK=full requires building with the full-grammar-pack feature"
            )
        }
        #[cfg(feature = "full-grammar-pack")]
        GrammarPack::Full => ServiceDependencies::new(
            Arc::new(FileArtifactPersistence),
            Arc::new(GitCommandRepository),
            discovery,
            Arc::new(SqliteRepositoryFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreFirstGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreFirstGrammarProvider)),
        ),
    }
}

/// Reopens and closes the durable store after all service readers have stopped.
///
/// This checkpoints `SQLite` WAL contents and removes writer sidecars so callers can safely copy
/// the database as an immutable snapshot.
///
/// # Errors
///
/// Returns a store error when the database cannot be opened or checkpointed.
pub fn quiesce_database(path: &Path) -> Result<(), StoreError> {
    drop(Store::open(path)?);
    Ok(())
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
        Self::with_watcher(services, true)
    }

    /// Creates one runtime without seeding or starting the background watcher.
    ///
    /// This mode is intended for short-lived, read-only processes. Indexing
    /// entry points must use [`Self::new`] so newly indexed projects remain
    /// watched after the request completes.
    #[must_use]
    pub fn new_query_only(services: Services) -> Self {
        Self::with_watcher(services, false)
    }

    fn with_watcher(services: Services, enabled: bool) -> Self {
        let watcher = Arc::new(Watcher::new(
            WatcherConfig::default(),
            ServiceIndexer::new(services.clone()),
        ));
        let watch_runtime = if enabled {
            if let Ok(projects) = services.list_projects() {
                for project in projects {
                    let _ = watcher.watch(project.project, project.root_path);
                }
            }
            watcher.spawn().ok()
        } else {
            None
        };
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
        let watcher_enabled = watcher_enabled_from_value(env::var_os(WATCHER_ENABLED_ENV));
        Services::from_env(service_dependencies()).map(|services| {
            if watcher_enabled {
                Self::new(services)
            } else {
                Self::new_query_only(services)
            }
        })
    }

    /// Creates one read-only runtime from process environment configuration.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration or recovery error when services cannot be opened.
    pub fn from_env_query_only() -> Result<Self, ServiceError> {
        Services::from_env(service_dependencies()).map(Self::new_query_only)
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        GrammarPack, configured_grammar_pack, path_is_included, service_dependencies_for_pack,
        watcher_enabled_from_value,
    };

    #[test]
    fn full_grammar_pack_is_selected_explicitly() {
        assert_eq!(configured_grammar_pack(Some("full")), GrammarPack::Full);
        assert_eq!(configured_grammar_pack(Some("FULL")), GrammarPack::Full);
        assert_eq!(configured_grammar_pack(None), GrammarPack::Core);
        assert_eq!(configured_grammar_pack(Some("core")), GrammarPack::Core);
    }

    #[test]
    fn configured_include_paths_match_relative_files_and_trees() {
        let includes = [
            Path::new("spring-core/src/main/java/example/StringUtils.java").to_path_buf(),
            Path::new("spring-core/src/test/java").to_path_buf(),
        ];

        assert!(path_is_included(
            Path::new("spring-core/src/main/java/example/StringUtils.java"),
            &includes
        ));
        assert!(!path_is_included(
            Path::new("spring-core/src/main/java/example/Other.java"),
            &includes
        ));
        assert!(path_is_included(
            Path::new("spring-core/src/test/java/example/StringUtilsTests.java"),
            &includes
        ));
        assert!(!path_is_included(
            Path::new("spring-core/src/testFixtures/java/example/StringUtilsTests.java"),
            &includes
        ));
    }

    #[test]
    fn watcher_is_enabled_by_default_and_can_be_disabled_explicitly() {
        assert!(watcher_enabled_from_value(None));
        assert!(watcher_enabled_from_value(Some("1".into())));
        assert!(watcher_enabled_from_value(Some("true".into())));
        assert!(!watcher_enabled_from_value(Some("0".into())));
        assert!(!watcher_enabled_from_value(Some("false".into())));
        assert!(!watcher_enabled_from_value(Some("OFF".into())));
        assert!(!watcher_enabled_from_value(Some(" disabled ".into())));
    }

    #[cfg(feature = "full-grammar-pack")]
    #[test]
    fn compiled_full_grammar_pack_constructs_dependencies() {
        let _dependencies = service_dependencies_for_pack(GrammarPack::Full);
    }

    #[cfg(not(feature = "full-grammar-pack"))]
    #[test]
    #[should_panic(
        expected = "GOLDENEYE_GRAMMAR_PACK=full requires building with the full-grammar-pack feature"
    )]
    fn unavailable_full_grammar_pack_fails_loudly() {
        service_dependencies_for_pack(GrammarPack::Full);
    }
}
