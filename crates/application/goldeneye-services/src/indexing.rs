use std::{path::Path, path::PathBuf, sync::Arc};

use goldeneye_index::{
    IndexError, IndexMode, IndexOptions, IndexResult, IndexService, IndexStatus,
    project_id_for_name,
};
use goldeneye_ports::GitPortError;
use serde::{Deserialize, Serialize};

use crate::{CancellationToken, ServiceError, Services};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRepositoryRequest {
    pub repo_path: PathBuf,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub mode: IndexRepositoryMode,
    #[serde(default)]
    pub persistence: bool,
}

impl IndexRepositoryRequest {
    #[must_use]
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
            name: None,
            mode: IndexRepositoryMode::default(),
            persistence: false,
        }
    }

    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    #[must_use]
    pub const fn with_mode(mut self, mode: IndexRepositoryMode) -> Self {
        self.mode = mode;
        self
    }

    #[must_use]
    pub const fn with_persistence(mut self, persistence: bool) -> Self {
        self.persistence = persistence;
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexRepositoryMode {
    #[default]
    Full,
    Moderate,
    Fast,
}

impl IndexRepositoryMode {
    const fn discovery(self) -> IndexMode {
        match self {
            Self::Full => IndexMode::Full,
            Self::Moderate => IndexMode::Moderate,
            Self::Fast => IndexMode::Fast,
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

impl Services {
    /// Indexes a repository using the requested full, moderate, or fast mode.
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

    /// Indexes a repository using the requested mode with cancellation and progress hooks.
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
        self.import_artifact_if_available(&root, hooks);
        hooks.report("opening_store");
        self.prepare_database()?;
        let store = self
            .dependencies
            .repositories()
            .open_index(&self.config.database_path)
            .map_err(ServiceError::Repository)?;
        let options = index_options(request, hooks)?;
        let mut index = IndexService::new(
            store,
            self.dependencies.index_syntax(),
            options,
            self.dependencies.discovery(),
        );
        hooks.report("indexing");
        let mut result = index
            .index_repository(root.clone())
            .map_err(map_index_error)?;
        drop(index);
        self.refresh_index_metadata(&mut result, &root, request.mode, hooks)?;
        self.export_artifact_if_requested(&result, &root, request.persistence, hooks)?;
        hooks.report("complete");
        Ok(index_repository_result(result))
    }

    fn import_artifact_if_available(&self, root: &Path, hooks: &OperationHooks) {
        if !self.config.database_path.is_file() && self.dependencies.artifact().exists(root) {
            hooks.report("importing_artifact");
            let _ = self
                .dependencies
                .artifact()
                .import(root, &self.config.database_path);
        }
    }

    fn refresh_index_metadata(
        &self,
        result: &mut IndexResult,
        root: &Path,
        mode: IndexRepositoryMode,
        hooks: &OperationHooks,
    ) -> Result<(), ServiceError> {
        self.refresh_git_history_counts(result, root, hooks)?;
        hooks.report("semantic_index");
        if let Err(error) =
            self.refresh_semantic_index_at(&result.project.id, result.project.generation, mode)
        {
            result.warnings.push(format!("semantic_index: {error}"));
        }
        Ok(())
    }

    fn refresh_git_history_counts(
        &self,
        result: &mut IndexResult,
        root: &Path,
        hooks: &OperationHooks,
    ) -> Result<(), ServiceError> {
        match self.refresh_git_history_at(&result.project.id, root, hooks.cancellation()) {
            Ok(history) if history.enriched_edges > 0 => {
                match self
                    .dependencies
                    .repositories()
                    .open_query(&self.config.database_path)
                    .and_then(|store| store.counts(&result.project.id))
                {
                    Ok(counts) => result.counts = counts,
                    Err(error) => result.warnings.push(format!("git_history_counts: {error}")),
                }
            }
            Ok(_) => {}
            Err(ServiceError::Git(GitPortError::Cancelled)) => {
                return Err(ServiceError::Cancelled);
            }
            Err(error) => result.warnings.push(format!("git_history: {error}")),
        }
        Ok(())
    }

    fn export_artifact_if_requested(
        &self,
        result: &IndexResult,
        root: &Path,
        persistence: bool,
        hooks: &OperationHooks,
    ) -> Result<(), ServiceError> {
        if persistence || self.dependencies.artifact().exists(root) {
            hooks.report("exporting_artifact");
            self.dependencies.artifact().export(
                &self.config.database_path,
                root,
                result.project.id.as_str(),
            )?;
        }
        Ok(())
    }
}

fn index_options(
    request: &IndexRepositoryRequest,
    hooks: &OperationHooks,
) -> Result<IndexOptions, ServiceError> {
    let mut options = IndexOptions::default();
    options.discovery.mode = request.mode.discovery();
    options.cancellation = hooks.cancellation.clone();
    options.project_id_override = request
        .name
        .as_deref()
        .filter(|name| !name.is_empty())
        .map(project_id_for_name)
        .transpose()
        .map_err(map_index_error)?;
    Ok(options)
}

fn index_repository_result(result: IndexResult) -> IndexRepositoryResult {
    IndexRepositoryResult {
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
    }
}

fn map_index_error(error: IndexError) -> ServiceError {
    if matches!(error, IndexError::Cancelled) {
        ServiceError::Cancelled
    } else {
        ServiceError::Index(error)
    }
}
