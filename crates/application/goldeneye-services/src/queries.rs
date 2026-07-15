use std::sync::Arc;

use crate::{
    ArchitectureRequest, ArchitectureResult, CodeSnippetRequest, CodeSnippetResult,
    GraphSchemaRequest, GraphSchemaResult, IndexStatusRequest, IndexStatusResult, ProjectId,
    ProjectSummary, QueryGraphRequest, QueryGraphResult, SearchCodeRequest, SearchCodeResult,
    SearchGraphPage, SearchGraphRequest, SemanticSearchRequest, SemanticSearchResult, ServiceError,
    Services, SimilaritySearchRequest, SimilaritySearchResult, TracePathRequest, TracePathResult,
};

impl Services {
    /// Lists persisted projects.
    ///
    /// # Errors
    ///
    /// Returns a typed storage/query failure.
    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, ServiceError> {
        self.with_query_engine(goldeneye_query::QueryEngine::list_projects)
    }

    /// Deletes one persisted project and all project-scoped graph data.
    ///
    /// # Errors
    ///
    /// Returns a storage failure. A missing database or project returns `false`.
    pub fn delete_project(&self, project: &ProjectId) -> Result<bool, ServiceError> {
        if !self.config.database_path.is_file() {
            return Ok(false);
        }
        let mut repository = self
            .dependencies
            .repositories()
            .open_project_administration(&self.config.database_path)
            .map_err(ServiceError::Repository)?;
        let deleted = repository
            .delete_project(project)
            .map_err(ServiceError::Repository)?;
        if deleted {
            self.query.invalidate_project(project);
        }
        Ok(deleted)
    }

    /// Rebuilds derived cross-project route and channel edges for all persisted projects.
    ///
    /// # Errors
    ///
    /// Returns graph identity, storage, or derived-edge limit failures.
    pub fn rebuild_cross_repo_intelligence(
        &self,
    ) -> Result<goldeneye_crosslink::CrossLinkOutcome, ServiceError> {
        self.prepare_database()?;
        let mut repository = self
            .dependencies
            .repositories()
            .open_crosslink(&self.config.database_path)
            .map_err(ServiceError::Repository)?;
        let outcome = goldeneye_crosslink::rebuild(&mut repository);
        self.query.invalidate_all();
        Ok(outcome?)
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
        self.with_query_engine(|engine| engine.index_status(request))
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
        self.with_query_engine(|engine| engine.graph_schema(request))
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
        self.with_query_engine(|engine| engine.search_graph(request))
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
        self.with_query_engine(|engine| engine.search_code(request))
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
        self.with_query_engine(|engine| engine.semantic_search(request))
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
        self.with_query_engine(|engine| engine.similarity_search(request))
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
        self.with_query_engine(|engine| engine.query_graph(request))
    }

    /// Traces graph relationships from one symbol.
    ///
    /// # Errors
    ///
    /// Returns typed validation, symbol resolution, storage, or query failures.
    pub fn trace_path(&self, request: &TracePathRequest) -> Result<TracePathResult, ServiceError> {
        self.with_query_engine(|engine| engine.trace_path(request))
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
        self.with_query_engine(|engine| engine.get_code_snippet(request))
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
        self.with_query_engine(|engine| engine.get_architecture(request))
    }

    fn with_query_engine<T>(
        &self,
        action: impl FnOnce(&goldeneye_query::QueryEngine) -> Result<T, goldeneye_query::QueryError>,
    ) -> Result<T, ServiceError> {
        let mut engine = self
            .query_engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if engine.is_none() {
            self.prepare_database()?;
            let repository = self
                .dependencies
                .repositories()
                .open_query(&self.config.database_path)
                .map_err(ServiceError::Repository)?;
            *engine = Some(goldeneye_query::QueryEngine::with_cache(
                repository,
                Arc::clone(&self.query),
            ));
        }
        Ok(action(engine.as_ref().expect("query engine initialized"))?)
    }
}
