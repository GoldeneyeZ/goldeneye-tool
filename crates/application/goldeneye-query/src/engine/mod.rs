mod architecture;
mod cache;
mod graph;
mod resolve;
mod search;
mod snippet;
mod trace;

pub use cache::QueryCache;
use graph::ProjectGraph;
pub(crate) use graph::{degrees, node_summary};
use resolve::resolve_symbol_in_graph;
pub(crate) use resolve::{ResolveMode, resolve_symbol};
use trace::{TraceCacheKey, trace_breadth_first};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use goldeneye_domain::{Generation, ProjectId};
use goldeneye_ports::QueryRepository;

use crate::types::{
    ArchitectureRequest, ArchitectureResult, CodeSnippetRequest, CodeSnippetResult,
    GraphSchemaRequest, GraphSchemaResult, IndexStatusRequest, IndexStatusResult, ProjectSummary,
    QueryError, QueryGraphRequest, QueryGraphResult, SchemaEntry, SearchCodeRequest,
    SearchCodeResult, SearchGraphPage, SearchGraphRequest, SemanticSearchRequest,
    SemanticSearchResult, SimilaritySearchRequest, SimilaritySearchResult, TracePathRequest,
    TracePathResult,
};

const MAX_TRACE_DEPTH: usize = 5;
const MAX_TRACE_LIMIT: usize = 1_000;

pub struct QueryEngine {
    repository: Box<dyn QueryRepository>,
    cache: Arc<QueryCache>,
}

impl QueryEngine {
    #[must_use]
    pub fn new(repository: impl QueryRepository + 'static) -> Self {
        Self::with_cache(repository, Arc::new(QueryCache::default()))
    }

    #[must_use]
    pub fn with_cache(repository: impl QueryRepository + 'static, cache: Arc<QueryCache>) -> Self {
        Self {
            repository: Box::new(repository),
            cache,
        }
    }

    /// Lists indexed projects in bytewise project-ID order.
    ///
    /// # Errors
    ///
    /// Returns a query error when the registry cannot be read.
    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, QueryError> {
        self.repository
            .list_projects()?
            .into_iter()
            .map(|project| {
                Ok(ProjectSummary {
                    project: project.id.as_str().to_owned(),
                    root_path: project.root_path,
                    generation: project.generation.value(),
                })
            })
            .collect()
    }

    /// Returns one project's persisted graph status.
    ///
    /// # Errors
    ///
    /// Returns a query error when the project is absent or the graph cannot be read.
    pub fn index_status(
        &self,
        request: &IndexStatusRequest,
    ) -> Result<IndexStatusResult, QueryError> {
        let project = self.require_project(&request.project)?;
        let counts = self.repository.counts(&request.project)?;
        let settings = self.repository.connection_settings()?;
        Ok(IndexStatusResult {
            project: project.id.as_str().to_owned(),
            root_path: project.root_path,
            generation: project.generation.value(),
            files: counts.files,
            nodes: counts.nodes,
            edges: counts.edges,
            query_only: settings.query_only,
        })
    }

    /// Derives deterministic node-label and edge-kind schema metadata.
    ///
    /// # Errors
    ///
    /// Returns a query error when the project is absent or the graph cannot be read.
    pub fn graph_schema(
        &self,
        request: &GraphSchemaRequest,
    ) -> Result<GraphSchemaResult, QueryError> {
        let graph = self.cached_graph(&request.project)?;
        let schema = self.repository.schema_info()?;
        Ok(GraphSchemaResult {
            project: request.project.as_str().to_owned(),
            schema_version: schema.version,
            node_labels: schema_entries(
                graph
                    .nodes
                    .iter()
                    .map(|node| (node.label.as_str(), node.properties.keys())),
            ),
            edge_types: schema_entries(
                graph
                    .edges
                    .iter()
                    .map(|edge| (edge.kind.as_str(), edge.properties.keys())),
            ),
        })
    }

    /// Searches graph nodes with deterministic filtering and cursor pagination.
    ///
    /// # Errors
    ///
    /// Returns a query error for invalid filters/cursors, absent projects, or repository failures.
    pub fn search_graph(
        &self,
        request: &SearchGraphRequest,
    ) -> Result<SearchGraphPage, QueryError> {
        let graph = self.cached_graph(&request.project)?;
        search::execute(self.repository.as_ref(), request, &graph)
    }

    /// Traverses graph edges breadth-first with deterministic cycle suppression.
    ///
    /// # Errors
    ///
    /// Returns a query error for invalid bounds, ambiguous/missing symbols, or repository failures.
    pub fn trace_path(&self, request: &TracePathRequest) -> Result<TracePathResult, QueryError> {
        let graph = self.cached_graph(&request.project)?;
        if request.depth == 0 || request.depth > MAX_TRACE_DEPTH {
            return Err(QueryError::InvalidTraceDepth {
                actual: request.depth,
                maximum: MAX_TRACE_DEPTH,
            });
        }
        if request.limit == 0 || request.limit > MAX_TRACE_LIMIT {
            return Err(QueryError::InvalidTraceLimit {
                actual: request.limit,
                maximum: MAX_TRACE_LIMIT,
            });
        }
        let cache_key = TraceCacheKey::from(request);
        if let Some(result) = graph.cached_trace(&cache_key) {
            return Ok(result);
        }
        let origin =
            resolve_symbol_in_graph(&request.function_name, &graph, ResolveMode::Callable)?;
        let (paths, truncated) = trace_breadth_first(origin, &graph, request);
        let result = TracePathResult {
            project: request.project.as_str().to_owned(),
            origin: node_summary(origin, None, &graph.degrees, Vec::new()),
            direction: request.direction,
            paths,
            truncated,
        };
        graph.cache_trace(cache_key, result.clone());
        Ok(result)
    }

    /// Compatibility alias for the legacy `trace_call_path` tool name.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::trace_path`].
    pub fn trace_call_path(
        &self,
        request: &TracePathRequest,
    ) -> Result<TracePathResult, QueryError> {
        self.trace_path(request)
    }

    /// Resolves one symbol and returns its exact indexed byte span after a freshness check.
    ///
    /// # Errors
    ///
    /// Returns a query error for resolution, stale/missing source, corrupt spans, or bounds.
    pub fn get_code_snippet(
        &self,
        request: &CodeSnippetRequest,
    ) -> Result<CodeSnippetResult, QueryError> {
        let project = self.require_project(&request.project)?;
        let graph = self.cached_graph_at_generation(&request.project, project.generation)?;
        snippet::execute(self.repository.as_ref(), request, &project, &graph)
    }

    /// Returns a deterministic high-level graph architecture summary.
    ///
    /// # Errors
    ///
    /// Returns a query error when the project is absent or graph records cannot be read.
    pub fn get_architecture(
        &self,
        request: &ArchitectureRequest,
    ) -> Result<ArchitectureResult, QueryError> {
        let project = self.require_project(&request.project)?;
        let graph = self.cached_graph(&request.project)?;
        let summary = graph.architecture_summary();

        Ok(ArchitectureResult {
            project: project.id.as_str().to_owned(),
            root_path: project.root_path,
            generation: project.generation.value(),
            total_nodes: summary.total_nodes,
            total_edges: summary.total_edges,
            languages: summary.languages.clone(),
            modules: summary.modules.clone(),
            types: summary.types.clone(),
            entry_points: summary.entry_points.clone(),
            edge_types: summary.edge_types.clone(),
        })
    }

    /// Executes the supported read-only Cypher subset over one project's graph.
    ///
    /// # Errors
    ///
    /// Returns a query error for mutation attempts, unsupported/syntax-invalid queries, absent
    /// projects, invalid row bounds, or repository failures.
    pub fn query_graph(&self, request: &QueryGraphRequest) -> Result<QueryGraphResult, QueryError> {
        let graph = self.cached_graph(&request.project)?;
        crate::cypher::execute(request, &graph.nodes, &graph.edges, &graph.degrees)
    }

    /// Searches indexed source and collapses line matches to their tightest graph nodes.
    ///
    /// # Errors
    ///
    /// Returns a query error for invalid patterns, missing projects, unsafe paths, or source I/O.
    pub fn search_code(&self, request: &SearchCodeRequest) -> Result<SearchCodeResult, QueryError> {
        crate::search_code::execute(self.repository.as_ref(), request)
    }

    /// Ranks callable/type nodes by the minimum cosine across semantic keywords.
    ///
    /// # Errors
    ///
    /// Returns a query error for invalid bounds, missing projects, or corrupt stored vectors.
    pub fn semantic_search(
        &self,
        request: &SemanticSearchRequest,
    ) -> Result<SemanticSearchResult, QueryError> {
        crate::semantic_query::semantic_search(self.repository.as_ref(), request)
    }

    /// Finds nodes whose persisted weighted-MinHash signature resembles one symbol.
    ///
    /// # Errors
    ///
    /// Returns a query error for resolution, bounds, missing signatures, or corrupt artifacts.
    pub fn similarity_search(
        &self,
        request: &SimilaritySearchRequest,
    ) -> Result<SimilaritySearchResult, QueryError> {
        crate::semantic_query::similarity_search(self.repository.as_ref(), request)
    }

    /// Compatibility alias with the agent-facing operation name.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::similarity_search`].
    pub fn find_similar(
        &self,
        request: &SimilaritySearchRequest,
    ) -> Result<SimilaritySearchResult, QueryError> {
        self.similarity_search(request)
    }

    fn require_project(
        &self,
        project: &ProjectId,
    ) -> Result<goldeneye_domain::ProjectRecord, QueryError> {
        self.repository
            .get_project(project)?
            .ok_or_else(|| QueryError::ProjectNotFound(project.clone()))
    }

    fn cached_graph(&self, project: &ProjectId) -> Result<Arc<ProjectGraph>, QueryError> {
        loop {
            let before = self.require_project(project)?.generation;
            if let Some(graph) = self.cache.get(project, before) {
                return Ok(graph);
            }
            let graph = self.cache.get_or_load(project, before, || {
                Ok((
                    self.repository.list_nodes(project)?,
                    self.repository.list_edges(project)?,
                ))
            })?;
            let after = self.require_project(project)?.generation;
            if before == after {
                return Ok(graph);
            }
        }
    }

    fn cached_graph_at_generation(
        &self,
        project: &ProjectId,
        generation: Generation,
    ) -> Result<Arc<ProjectGraph>, QueryError> {
        if let Some(graph) = self.cache.get(project, generation) {
            return Ok(graph);
        }
        let graph = self.cache.get_or_load(project, generation, || {
            Ok((
                self.repository.list_nodes(project)?,
                self.repository.list_edges(project)?,
            ))
        })?;
        if self.require_project(project)?.generation == generation {
            return Ok(graph);
        }
        self.cached_graph(project)
    }
}

fn schema_entries<'a, I, K>(items: I) -> Vec<SchemaEntry>
where
    I: IntoIterator<Item = (&'a str, K)>,
    K: Iterator<Item = &'a String>,
{
    let mut entries: BTreeMap<String, (u64, BTreeSet<String>)> = BTreeMap::new();
    for (name, properties) in items {
        let entry = entries.entry(name.to_owned()).or_default();
        entry.0 += 1;
        entry.1.extend(properties.cloned());
    }
    entries
        .into_iter()
        .map(|(name, (count, properties))| SchemaEntry {
            name,
            count,
            properties: properties.into_iter().collect(),
        })
        .collect()
}
