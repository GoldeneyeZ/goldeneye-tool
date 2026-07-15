#![forbid(unsafe_code)]

//! Tool-neutral orchestration over Goldeneye indexing, storage, and query crates.

mod adr_traces;
mod configuration;
mod dependencies;
mod edit;
mod error;
mod git;
mod indexing;
mod queries;
mod semantic_index;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub use adr_traces::{
    ADR_EMPTY_HINT, IngestTracesRequest, IngestTracesResult, MAX_PERSISTED_TRACE_BATCH,
    MAX_TRACE_ENDPOINT_BYTES, ManageAdrRequest, ManageAdrResult, parse_runtime_traces,
};
pub use configuration::{
    ALLOWED_ROOT_ENV, DATABASE_PATH_ENV, DEFAULT_SEMANTIC_THRESHOLD, PROJECT_ROOT_ENV,
    SEMANTIC_ENABLED_ENV, SEMANTIC_THRESHOLD_ENV, ServiceConfig,
};
pub use dependencies::ServiceDependencies;
pub use edit::{
    CreateFileRequest, DeleteNodeRequest, EditMutationResult, EditParsePolicy, GraphMutation,
    InspectSyntaxRequest, InspectSyntaxResult, InspectionSize, MutationDiagnostics, MutationDiff,
    MutationSize, NodeContentRequest, SyntaxDiagnosticResult,
};
pub use error::{ServiceError, ServiceErrorCode};
pub use git::{
    DEFAULT_CHANGE_DEPTH, DetectChangesRequest, DetectChangesResult, GitHistoryResult,
    ImpactedSymbol, MAX_CHANGE_DEPTH, MAX_IMPACTED_SYMBOLS,
};
pub use indexing::{
    IndexRepositoryMode, IndexRepositoryRequest, IndexRepositoryResult, IndexRepositoryStatus,
    OperationHooks, ProgressEvent,
};

pub use goldeneye_domain::{Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath};
pub use goldeneye_edit::{RecoveryAction, RecoveryEntry, RecoveryReport};
pub use goldeneye_index::CancellationToken;
pub use goldeneye_ports::GitContext;
pub use goldeneye_query::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, EdgeSummary, GraphSchemaRequest, GraphSchemaResult,
    IndexStatusRequest, IndexStatusResult, NodeSummary, PageRequest, ProjectSummary, QueryError,
    QueryGraphRequest, QueryGraphResult, QueryValue, SchemaEntry, SearchCodeFilesResult,
    SearchCodeHit, SearchCodeMatchesResult, SearchCodeMode, SearchCodeRequest, SearchCodeResult,
    SearchGraphPage, SearchGraphRequest, SemanticSearchHit, SemanticSearchRequest,
    SemanticSearchResult, SimilaritySearchHit, SimilaritySearchRequest, SimilaritySearchResult,
    TraceDirection, TraceHop, TracePathRequest, TracePathResult,
};

#[derive(Clone)]
pub struct Services {
    config: ServiceConfig,
    dependencies: ServiceDependencies,
    edit: Arc<Mutex<Option<goldeneye_edit::DurableEditService>>>,
    query: Arc<goldeneye_query::QueryCache>,
    query_engine: Arc<Mutex<Option<goldeneye_query::QueryEngine>>>,
}

impl std::fmt::Debug for Services {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Services")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl Services {
    #[must_use]
    pub fn new(config: ServiceConfig, dependencies: ServiceDependencies) -> Self {
        Self {
            config,
            dependencies,
            edit: Arc::new(Mutex::new(None)),
            query: Arc::new(goldeneye_query::QueryCache::default()),
            query_engine: Arc::new(Mutex::new(None)),
        }
    }

    /// Builds lazy services from process environment.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration error when environment resolution fails.
    pub fn from_env(dependencies: ServiceDependencies) -> Result<Self, ServiceError> {
        let (services, recovery) = Self::open(ServiceConfig::from_env()?, dependencies)?;
        Self::ensure_recovery_resolved(&recovery)?;
        Ok(services)
    }

    #[must_use]
    pub const fn config(&self) -> &ServiceConfig {
        &self.config
    }

    fn prepare_database(&self) -> Result<(), ServiceError> {
        if let Some(parent) = self.config.database_path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|source| ServiceError::DatabaseDirectory {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        if !self.config.database_path.exists() {
            self.dependencies
                .repositories()
                .initialize(&self.config.database_path)
                .map_err(ServiceError::Repository)?;
        }
        Ok(())
    }

    fn resolve_repository(&self, path: &Path) -> Result<PathBuf, ServiceError> {
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.config.project_root.join(path)
        };
        let canonical =
            candidate
                .canonicalize()
                .map_err(|source| ServiceError::InvalidRepositoryPath {
                    path: candidate.clone(),
                    source,
                })?;
        if !canonical.is_dir() {
            return Err(ServiceError::RepositoryNotDirectory(canonical));
        }
        if let Some(allowed_root) = &self.config.allowed_root {
            let allowed = allowed_root.canonicalize().map_err(|source| {
                ServiceError::InvalidRepositoryPath {
                    path: allowed_root.clone(),
                    source,
                }
            })?;
            if !canonical.starts_with(allowed) {
                return Err(ServiceError::OutsideAllowedRoot);
            }
        }
        Ok(canonical)
    }
}
