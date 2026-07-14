#![forbid(unsafe_code)]

//! Tool-neutral orchestration over Goldeneye indexing, storage, and query crates.

mod adr_traces;
mod edit;
mod git;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use goldeneye_discovery::IndexMode;
use goldeneye_index::{IndexError, IndexOptions, IndexService, IndexStatus};
use goldeneye_store::{Store, StoreError};
use goldeneye_syntax::CoreGrammarProvider;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use adr_traces::{
    ADR_EMPTY_HINT, IngestTracesRequest, IngestTracesResult, MAX_PERSISTED_TRACE_BATCH,
    MAX_TRACE_ENDPOINT_BYTES, ManageAdrRequest, ManageAdrResult, parse_runtime_traces,
};
pub use edit::{
    CreateFileRequest, DeleteNodeRequest, EditMutationResult, EditParsePolicy, GraphMutation,
    InspectSyntaxRequest, InspectSyntaxResult, InspectionSize, MutationDiagnostics, MutationDiff,
    MutationSize, NodeContentRequest, SyntaxDiagnosticResult,
};
pub use git::{
    DEFAULT_CHANGE_DEPTH, DetectChangesRequest, DetectChangesResult, GitHistoryResult,
    ImpactedSymbol, MAX_CHANGE_DEPTH, MAX_IMPACTED_SYMBOLS,
};
pub use goldeneye_domain::{Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath};
pub use goldeneye_edit::{RecoveryAction, RecoveryEntry, RecoveryReport};
pub use goldeneye_git::GitContext;
pub use goldeneye_index::CancellationToken;
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

pub const DATABASE_PATH_ENV: &str = "GOLDENEYE_DB_PATH";
pub const PROJECT_ROOT_ENV: &str = "GOLDENEYE_PROJECT_ROOT";
pub const ALLOWED_ROOT_ENV: &str = "CBM_ALLOWED_ROOT";
pub const SEMANTIC_ENABLED_ENV: &str = "CBM_SEMANTIC_ENABLED";
pub const SEMANTIC_THRESHOLD_ENV: &str = "CBM_SEMANTIC_THRESHOLD";
pub const DEFAULT_SEMANTIC_THRESHOLD: f32 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    database_path: PathBuf,
    project_root: PathBuf,
    allowed_root: Option<PathBuf>,
    semantic_enabled: bool,
    semantic_threshold: f32,
}

impl ServiceConfig {
    #[must_use]
    pub fn new(database_path: impl Into<PathBuf>, project_root: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
            project_root: project_root.into(),
            allowed_root: None,
            semantic_enabled: false,
            semantic_threshold: DEFAULT_SEMANTIC_THRESHOLD,
        }
    }

    #[must_use]
    pub fn with_allowed_root(mut self, allowed_root: impl Into<PathBuf>) -> Self {
        self.allowed_root = Some(allowed_root.into());
        self
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    #[must_use]
    pub fn allowed_root(&self) -> Option<&Path> {
        self.allowed_root.as_deref()
    }

    #[must_use]
    pub const fn semantic_enabled(&self) -> bool {
        self.semantic_enabled
    }

    #[must_use]
    pub const fn semantic_threshold(&self) -> f32 {
        self.semantic_threshold
    }

    #[must_use]
    pub fn with_semantic_config(mut self, enabled: bool, threshold: f32) -> Self {
        self.semantic_enabled = enabled;
        self.semantic_threshold = valid_semantic_threshold(threshold)
            .then_some(threshold)
            .unwrap_or(DEFAULT_SEMANTIC_THRESHOLD);
        self
    }

    /// Builds configuration from process environment without opening the database.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration error when the current directory cannot be read.
    pub fn from_env() -> Result<Self, ServiceError> {
        let project_root = env::var_os(PROJECT_ROOT_ENV).map_or_else(
            || env::current_dir().map_err(|source| ServiceError::CurrentDirectory { source }),
            |value| Ok(PathBuf::from(value)),
        )?;
        let database_path =
            env::var_os(DATABASE_PATH_ENV).map_or_else(default_database_path, PathBuf::from);
        let mut config = Self::new(database_path, project_root).with_semantic_config(
            semantic_enabled_from_value(env::var_os(SEMANTIC_ENABLED_ENV)),
            semantic_threshold_from_value(env::var_os(SEMANTIC_THRESHOLD_ENV)),
        );
        if let Some(value) = env::var_os(ALLOWED_ROOT_ENV) {
            config = config.with_allowed_root(PathBuf::from(value));
        }
        Ok(config)
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self::from_env().unwrap_or_else(|_| Self::new(default_database_path(), "."))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRepositoryRequest {
    pub repo_path: PathBuf,
}

impl IndexRepositoryRequest {
    #[must_use]
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexRepositoryStatus {
    Indexed,
    Unchanged,
    RejectedSyntax,
}

impl IndexRepositoryStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Indexed => "indexed",
            Self::Unchanged => "unchanged",
            Self::RejectedSyntax => "rejected_syntax",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRepositoryResult {
    pub project: String,
    pub root_path: String,
    pub status: IndexRepositoryStatus,
    pub generation: u64,
    pub discovered_files: usize,
    pub new_files: usize,
    pub changed_files: usize,
    pub deleted_files: usize,
    pub unchanged_files: usize,
    pub parsed_files: usize,
    pub reused_files: usize,
    pub files: u64,
    pub nodes: u64,
    pub edges: u64,
    pub diagnostics: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub stage: String,
}

type ProgressCallback = dyn Fn(ProgressEvent) + Send + Sync + 'static;

#[derive(Clone)]
pub struct OperationHooks {
    cancellation: CancellationToken,
    progress: Option<Arc<ProgressCallback>>,
}

impl OperationHooks {
    #[must_use]
    pub const fn new(cancellation: CancellationToken) -> Self {
        Self {
            cancellation,
            progress: None,
        }
    }

    #[must_use]
    pub fn with_progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(ProgressEvent) + Send + Sync + 'static,
    {
        self.progress = Some(Arc::new(callback));
        self
    }

    #[must_use]
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    fn report(&self, stage: &str) {
        if let Some(callback) = &self.progress {
            callback(ProgressEvent {
                stage: stage.to_owned(),
            });
        }
    }
}

impl Default for OperationHooks {
    fn default() -> Self {
        Self::new(CancellationToken::new())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceErrorCode {
    Configuration,
    InvalidInput,
    Forbidden,
    NotFound,
    Cancelled,
    Storage,
    Index,
    Query,
    Conflict,
}

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("cannot read current directory: {source}")]
    CurrentDirectory {
        #[source]
        source: std::io::Error,
    },
    #[error("cannot create database directory {path}: {source}")]
    DatabaseDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("repository path does not exist or cannot be resolved: {path}: {source}")]
    InvalidRepositoryPath {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("repository root is not a directory: {0}")]
    RepositoryNotDirectory(PathBuf),
    #[error("repo_path is outside the allowed root")]
    OutsideAllowedRoot,
    #[error("index operation was cancelled")]
    Cancelled,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Index(IndexError),
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Git(#[from] goldeneye_git::GitError),
    #[error("{message}")]
    Edit {
        code: ServiceErrorCode,
        message: String,
    },
}

impl ServiceError {
    #[must_use]
    pub const fn code(&self) -> ServiceErrorCode {
        match self {
            Self::CurrentDirectory { .. } | Self::DatabaseDirectory { .. } => {
                ServiceErrorCode::Configuration
            }
            Self::InvalidRepositoryPath { .. } | Self::RepositoryNotDirectory(_) => {
                ServiceErrorCode::InvalidInput
            }
            Self::OutsideAllowedRoot => ServiceErrorCode::Forbidden,
            Self::Cancelled | Self::Index(IndexError::Cancelled) => ServiceErrorCode::Cancelled,
            Self::Store(_) => ServiceErrorCode::Storage,
            Self::Index(_) => ServiceErrorCode::Index,
            Self::Query(QueryError::ProjectNotFound(_)) => ServiceErrorCode::NotFound,
            Self::Query(_) => ServiceErrorCode::Query,
            Self::Git(goldeneye_git::GitError::Cancelled) => ServiceErrorCode::Cancelled,
            Self::Git(goldeneye_git::GitError::InvalidReference) => ServiceErrorCode::InvalidInput,
            Self::Git(_) => ServiceErrorCode::Index,
            Self::Edit { code, .. } => *code,
        }
    }
}

#[derive(Clone)]
pub struct Services {
    config: ServiceConfig,
    edit: Arc<Mutex<Option<goldeneye_edit::DurableEditService<CoreGrammarProvider>>>>,
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
    pub fn new(config: ServiceConfig) -> Self {
        Self {
            config,
            edit: Arc::new(Mutex::new(None)),
        }
    }

    /// Builds lazy services from process environment.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration error when environment resolution fails.
    pub fn from_env() -> Result<Self, ServiceError> {
        let (services, recovery) = Self::open(ServiceConfig::from_env()?)?;
        Self::ensure_recovery_resolved(&recovery)?;
        Ok(services)
    }

    #[must_use]
    pub const fn config(&self) -> &ServiceConfig {
        &self.config
    }

    /// Indexes a repository in fast mode.
    ///
    /// # Errors
    ///
    /// Returns typed path-policy, cancellation, indexing, or storage failures.
    pub fn index_repository(
        &self,
        request: &IndexRepositoryRequest,
    ) -> Result<IndexRepositoryResult, ServiceError> {
        self.index_repository_with_hooks(request, &OperationHooks::default())
    }

    /// Indexes a repository in fast mode with cancellation and progress hooks.
    ///
    /// # Errors
    ///
    /// Returns typed path-policy, cancellation, indexing, or storage failures.
    pub fn index_repository_with_hooks(
        &self,
        request: &IndexRepositoryRequest,
        hooks: &OperationHooks,
    ) -> Result<IndexRepositoryResult, ServiceError> {
        if hooks.cancellation.is_cancelled() {
            return Err(ServiceError::Cancelled);
        }
        hooks.report("resolving");
        let root = self.resolve_repository(&request.repo_path)?;
        hooks.report("opening_store");
        self.prepare_database()?;
        let store = Store::open(&self.config.database_path)?;
        let mut options = IndexOptions::default();
        options.discovery.mode = IndexMode::Fast;
        options.cancellation = hooks.cancellation.clone();
        let mut index = IndexService::new(store, CoreGrammarProvider, options);
        hooks.report("indexing");
        let mut result = index
            .index_repository(root.clone())
            .map_err(map_index_error)?;
        drop(index);
        match self.refresh_git_history_at(&result.project.id, &root, hooks.cancellation()) {
            Ok(history) if history.enriched_edges > 0 => {
                match Store::open(&self.config.database_path)
                    .and_then(|store| store.counts(&result.project.id))
                {
                    Ok(counts) => result.counts = counts,
                    Err(error) => result.warnings.push(format!("git_history_counts: {error}")),
                }
            }
            Ok(_) => {}
            Err(ServiceError::Git(goldeneye_git::GitError::Cancelled)) => {
                return Err(ServiceError::Cancelled);
            }
            Err(error) => result.warnings.push(format!("git_history: {error}")),
        }
        hooks.report("complete");
        Ok(IndexRepositoryResult {
            project: result.project.id.as_str().to_owned(),
            root_path: result.project.root_path,
            status: match result.status {
                IndexStatus::Indexed => IndexRepositoryStatus::Indexed,
                IndexStatus::Unchanged => IndexRepositoryStatus::Unchanged,
                IndexStatus::RejectedSyntax => IndexRepositoryStatus::RejectedSyntax,
            },
            generation: result.project.generation.value(),
            discovered_files: result.discovered_files,
            new_files: result.new_files,
            changed_files: result.changed_files,
            deleted_files: result.deleted_files,
            unchanged_files: result.unchanged_files,
            parsed_files: result.parsed_files,
            reused_files: result.reused_files,
            files: result.counts.files,
            nodes: result.counts.nodes,
            edges: result.counts.edges,
            diagnostics: result.diagnostics.len(),
            warnings: result.warnings,
        })
    }

    /// Lists persisted projects.
    ///
    /// # Errors
    ///
    /// Returns a typed storage/query failure.
    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, ServiceError> {
        Ok(self.query_engine()?.list_projects()?)
    }

    /// Returns persisted index status for one project.
    ///
    /// # Errors
    ///
    /// Returns a typed not-found, storage, or query failure.
    pub fn index_status(
        &self,
        request: &IndexStatusRequest,
    ) -> Result<IndexStatusResult, ServiceError> {
        Ok(self.query_engine()?.index_status(request)?)
    }

    /// Returns graph schema information for one project.
    ///
    /// # Errors
    ///
    /// Returns a typed not-found, storage, or query failure.
    pub fn get_graph_schema(
        &self,
        request: &GraphSchemaRequest,
    ) -> Result<GraphSchemaResult, ServiceError> {
        Ok(self.query_engine()?.graph_schema(request)?)
    }

    /// Searches one project with bounded cursor pagination.
    ///
    /// # Errors
    ///
    /// Returns typed validation, not-found, storage, or query failures.
    pub fn search_graph(
        &self,
        request: &SearchGraphRequest,
    ) -> Result<SearchGraphPage, ServiceError> {
        Ok(self.query_engine()?.search_graph(request)?)
    }

    /// Searches indexed source and collapses matching lines to graph nodes.
    ///
    /// # Errors
    ///
    /// Returns typed validation, source, not-found, storage, or query failures.
    pub fn search_code(
        &self,
        request: &SearchCodeRequest,
    ) -> Result<SearchCodeResult, ServiceError> {
        Ok(self.query_engine()?.search_code(request)?)
    }

    /// Searches persisted semantic vectors using upstream minimum-cosine ranking.
    ///
    /// # Errors
    ///
    /// Returns typed validation, not-found, storage, or query failures.
    pub fn semantic_search(
        &self,
        request: &SemanticSearchRequest,
    ) -> Result<SemanticSearchResult, ServiceError> {
        Ok(self.query_engine()?.semantic_search(request)?)
    }

    /// Finds nodes with persisted structural signatures similar to one symbol.
    ///
    /// # Errors
    ///
    /// Returns typed validation, resolution, storage, or query failures.
    pub fn similarity_search(
        &self,
        request: &SimilaritySearchRequest,
    ) -> Result<SimilaritySearchResult, ServiceError> {
        Ok(self.query_engine()?.similarity_search(request)?)
    }

    /// Compatibility alias for [`Self::similarity_search`].
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`Self::similarity_search`].
    pub fn find_similar(
        &self,
        request: &SimilaritySearchRequest,
    ) -> Result<SimilaritySearchResult, ServiceError> {
        self.similarity_search(request)
    }

    /// Executes the supported read-only Cypher subset.
    ///
    /// # Errors
    ///
    /// Returns typed validation, not-found, storage, or query failures.
    pub fn query_graph(
        &self,
        request: &QueryGraphRequest,
    ) -> Result<QueryGraphResult, ServiceError> {
        Ok(self.query_engine()?.query_graph(request)?)
    }

    /// Traces graph relationships from one symbol.
    ///
    /// # Errors
    ///
    /// Returns typed validation, symbol resolution, storage, or query failures.
    pub fn trace_path(&self, request: &TracePathRequest) -> Result<TracePathResult, ServiceError> {
        Ok(self.query_engine()?.trace_path(request)?)
    }

    /// Compatibility alias for [`Self::trace_path`].
    ///
    /// # Errors
    ///
    /// Returns the same typed failures as [`Self::trace_path`].
    pub fn trace_call_path(
        &self,
        request: &TracePathRequest,
    ) -> Result<TracePathResult, ServiceError> {
        self.trace_path(request)
    }

    /// Returns bounded source for an exact or uniquely resolved symbol.
    ///
    /// # Errors
    ///
    /// Returns typed symbol, freshness, source, storage, or query failures.
    pub fn get_code_snippet(
        &self,
        request: &CodeSnippetRequest,
    ) -> Result<CodeSnippetResult, ServiceError> {
        Ok(self.query_engine()?.get_code_snippet(request)?)
    }

    /// Returns compact architecture summaries for one project.
    ///
    /// # Errors
    ///
    /// Returns typed not-found, storage, or query failures.
    pub fn get_architecture(
        &self,
        request: &ArchitectureRequest,
    ) -> Result<ArchitectureResult, ServiceError> {
        Ok(self.query_engine()?.get_architecture(request)?)
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
            drop(Store::open(&self.config.database_path)?);
        }
        Ok(())
    }

    fn query_engine(&self) -> Result<goldeneye_query::QueryEngine, ServiceError> {
        self.prepare_database()?;
        Ok(goldeneye_query::QueryEngine::open(
            &self.config.database_path,
        )?)
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

fn map_index_error(error: IndexError) -> ServiceError {
    if matches!(error, IndexError::Cancelled) {
        ServiceError::Cancelled
    } else {
        ServiceError::Index(error)
    }
}

fn default_database_path() -> PathBuf {
    if let Some(path) = env::var_os("CBM_CACHE_DIR") {
        return PathBuf::from(path).join("goldeneye.db");
    }
    if let Some(path) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path)
            .join("codebase-memory-mcp")
            .join("goldeneye.db");
    }
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path)
            .join("codebase-memory-mcp")
            .join("goldeneye.db");
    }
    if let Some(path) = env::var_os("HOME") {
        return PathBuf::from(path)
            .join(".cache")
            .join("codebase-memory-mcp")
            .join("goldeneye.db");
    }
    PathBuf::from(".goldeneye/goldeneye.db")
}

fn semantic_enabled_from_value(value: Option<std::ffi::OsString>) -> bool {
    value.is_some_and(|value| value.to_string_lossy().starts_with('1'))
}

fn semantic_threshold_from_value(value: Option<std::ffi::OsString>) -> f32 {
    value
        .and_then(|value| value.to_string_lossy().parse::<f32>().ok())
        .filter(|value| valid_semantic_threshold(*value))
        .unwrap_or(DEFAULT_SEMANTIC_THRESHOLD)
}

fn valid_semantic_threshold(value: f32) -> bool {
    value > 0.0 && value <= 1.0
}

#[cfg(test)]
mod semantic_config_tests {
    use std::ffi::OsString;

    use super::{
        DEFAULT_SEMANTIC_THRESHOLD, semantic_enabled_from_value, semantic_threshold_from_value,
    };

    #[test]
    fn upstream_semantic_environment_parsing_is_exact() {
        assert!(!semantic_enabled_from_value(None));
        assert!(!semantic_enabled_from_value(Some(OsString::from("true"))));
        assert!(semantic_enabled_from_value(Some(OsString::from("1"))));
        assert!(semantic_enabled_from_value(Some(OsString::from(
            "1-enabled"
        ))));

        assert_eq!(
            semantic_threshold_from_value(None),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("invalid"))),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("0"))),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("1.1"))),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("0.82"))),
            0.82
        );
    }
}
