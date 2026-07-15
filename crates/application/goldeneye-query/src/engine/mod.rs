mod resolve;
mod search;
mod snippet;
mod trace;

use resolve::resolve_symbol_in_graph;
pub(crate) use resolve::{ResolveMode, resolve_symbol};
use search::{MAX_CACHED_SEARCH_PAGES, SearchCacheKey};
use trace::trace_breadth_first;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, OnceLock};

use goldeneye_domain::{FileRecord, Generation, GraphEdge, GraphNode, NodeId, ProjectId};
use goldeneye_ports::QueryRepository;

use crate::types::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, GraphSchemaRequest, GraphSchemaResult, IndexStatusRequest,
    IndexStatusResult, NodeSummary, ProjectSummary, QueryError, QueryGraphRequest,
    QueryGraphResult, SchemaEntry, SearchCodeRequest, SearchCodeResult, SearchGraphPage,
    SearchGraphRequest, SemanticSearchRequest, SemanticSearchResult, SimilaritySearchRequest,
    SimilaritySearchResult, TraceDirection, TracePathRequest, TracePathResult,
};

const MAX_TRACE_DEPTH: usize = 5;
const MAX_TRACE_LIMIT: usize = 1_000;
const MAX_CACHED_TRACE_RESULTS: usize = 16;

pub struct QueryEngine {
    repository: Box<dyn QueryRepository>,
    cache: Arc<QueryCache>,
}

#[derive(Default)]
pub struct QueryCache {
    graphs: Mutex<BTreeMap<ProjectId, Arc<ProjectGraph>>>,
}

struct ProjectGraph {
    generation: u64,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    degrees: BTreeMap<NodeId, (usize, usize)>,
    edges_by_node: BTreeMap<NodeId, Vec<usize>>,
    define_counts: BTreeMap<NodeId, usize>,
    nodes_by_name: BTreeMap<String, Vec<usize>>,
    nodes_by_id: BTreeMap<NodeId, usize>,
    nodes_by_qualified_name: BTreeMap<String, usize>,
    search_pages: Mutex<BTreeMap<SearchCacheKey, SearchGraphPage>>,
    trace_results: Mutex<BTreeMap<TraceCacheKey, TracePathResult>>,
    files_by_path: Mutex<BTreeMap<String, FileRecord>>,
    architecture_summary: OnceLock<ArchitectureSummary>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct TraceCacheKey {
    function_name: String,
    direction: u8,
    depth: usize,
    limit: usize,
    edge_types: Vec<String>,
}

struct ArchitectureSummary {
    total_nodes: usize,
    total_edges: usize,
    languages: Vec<CountSummary>,
    modules: Vec<ArchitectureModule>,
    types: Vec<NodeSummary>,
    entry_points: Vec<NodeSummary>,
    edge_types: Vec<CountSummary>,
}

impl QueryCache {
    /// Drops one project's cached graph even when its durable generation is unchanged.
    pub fn invalidate_project(&self, project: &ProjectId) {
        self.graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(project);
    }

    /// Drops every cached project graph after a multi-project derived-graph write.
    pub fn invalidate_all(&self) {
        self.graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    fn get(&self, project: &ProjectId, generation: Generation) -> Option<Arc<ProjectGraph>> {
        self.graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(project)
            .filter(|graph| graph.generation == generation.value())
            .map(Arc::clone)
    }

    fn get_or_load(
        &self,
        project: &ProjectId,
        generation: Generation,
        mut load: impl FnMut() -> Result<(Vec<GraphNode>, Vec<GraphEdge>), QueryError>,
    ) -> Result<Arc<ProjectGraph>, QueryError> {
        let mut graphs = self
            .graphs
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(graph) = graphs
            .get(project)
            .filter(|graph| graph.generation == generation.value())
        {
            return Ok(Arc::clone(graph));
        }
        let (nodes, edges) = load()?;
        let graph = Arc::new(ProjectGraph::new(generation, nodes, edges));
        graphs.insert(project.clone(), Arc::clone(&graph));
        Ok(graph)
    }
}

impl ProjectGraph {
    fn new(generation: Generation, nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> Self {
        let degrees = degrees(&edges);
        let mut edges_by_node = BTreeMap::<NodeId, Vec<usize>>::new();
        let mut define_counts = BTreeMap::<NodeId, usize>::new();
        let mut nodes_by_name = BTreeMap::<String, Vec<usize>>::new();
        let mut nodes_by_id = BTreeMap::<NodeId, usize>::new();
        let mut nodes_by_qualified_name = BTreeMap::<String, usize>::new();
        for (index, node) in nodes.iter().enumerate() {
            nodes_by_name
                .entry(node.name.clone())
                .or_default()
                .push(index);
            nodes_by_id.insert(node.id.clone(), index);
            nodes_by_qualified_name.insert(node.qualified_name.as_str().to_owned(), index);
        }
        for (index, edge) in edges.iter().enumerate() {
            edges_by_node
                .entry(edge.source.clone())
                .or_default()
                .push(index);
            if edge.target != edge.source {
                edges_by_node
                    .entry(edge.target.clone())
                    .or_default()
                    .push(index);
            }
            if edge.kind.as_str() == "DEFINES" {
                *define_counts.entry(edge.source.clone()).or_default() += 1;
            }
        }
        Self {
            generation: generation.value(),
            nodes,
            edges,
            degrees,
            edges_by_node,
            define_counts,
            nodes_by_name,
            nodes_by_id,
            nodes_by_qualified_name,
            search_pages: Mutex::new(BTreeMap::new()),
            trace_results: Mutex::new(BTreeMap::new()),
            files_by_path: Mutex::new(BTreeMap::new()),
            architecture_summary: OnceLock::new(),
        }
    }

    fn cached_file(&self, path: &str) -> Option<FileRecord> {
        self.files_by_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(path)
            .cloned()
    }

    fn cache_file(&self, file: FileRecord) {
        self.files_by_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(file.id.path.as_str().to_owned(), file);
    }

    fn cached_search(&self, key: &SearchCacheKey) -> Option<SearchGraphPage> {
        self.search_pages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    fn cache_search(&self, key: SearchCacheKey, page: SearchGraphPage) {
        let mut pages = self
            .search_pages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if pages.len() >= MAX_CACHED_SEARCH_PAGES
            && !pages.contains_key(&key)
            && let Some(first) = pages.keys().next().cloned()
        {
            pages.remove(&first);
        }
        pages.insert(key, page);
    }

    fn cached_trace(&self, key: &TraceCacheKey) -> Option<TracePathResult> {
        self.trace_results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    fn cache_trace(&self, key: TraceCacheKey, result: TracePathResult) {
        let mut results = self
            .trace_results
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if results.len() >= MAX_CACHED_TRACE_RESULTS
            && !results.contains_key(&key)
            && let Some(first) = results.keys().next().cloned()
        {
            results.remove(&first);
        }
        results.insert(key, result);
    }

    fn architecture_summary(&self) -> &ArchitectureSummary {
        self.architecture_summary
            .get_or_init(|| ArchitectureSummary::from_graph(self))
    }
}

impl ArchitectureSummary {
    fn from_graph(graph: &ProjectGraph) -> Self {
        let nodes = &graph.nodes;
        let edges = &graph.edges;

        let mut languages: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for node in nodes {
            let Some(language) = node
                .properties
                .get("language")
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if let Some(path) = &node.file_path {
                languages
                    .entry(language.to_owned())
                    .or_default()
                    .insert(path.as_str().to_owned());
            }
        }
        let languages = languages
            .into_iter()
            .map(|(name, paths)| CountSummary {
                name,
                count: u64::try_from(paths.len()).unwrap_or(u64::MAX),
            })
            .collect();

        let modules = nodes
            .iter()
            .filter(|node| node.label.as_str() == "Module")
            .map(|node| ArchitectureModule {
                name: node.name.clone(),
                qualified_name: node.qualified_name.as_str().to_owned(),
                file_path: node.file_path.as_ref().map(|path| path.as_str().to_owned()),
                defined_symbols: graph.define_counts.get(&node.id).copied().unwrap_or(0),
            })
            .collect();
        let type_labels = [
            "Class",
            "Enum",
            "Interface",
            "Struct",
            "Trait",
            "Type",
            "TypeAlias",
        ];
        let types = nodes
            .iter()
            .filter(|node| type_labels.contains(&node.label.as_str()))
            .map(|node| node_summary(node, None, &graph.degrees, Vec::new()))
            .collect();
        let entry_points = nodes
            .iter()
            .filter(|node| {
                node.properties
                    .get("is_entry_point")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                    && !node
                        .properties
                        .get("is_test")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false)
                    && !node
                        .file_path
                        .as_ref()
                        .is_some_and(|path| path.as_str().to_lowercase().contains("test"))
            })
            .map(|node| node_summary(node, None, &graph.degrees, Vec::new()))
            .take(20)
            .collect();
        let mut edge_counts: BTreeMap<String, u64> = BTreeMap::new();
        for edge in edges {
            *edge_counts
                .entry(edge.kind.as_str().to_owned())
                .or_default() += 1;
        }
        let edge_types = edge_counts
            .into_iter()
            .map(|(name, count)| CountSummary { name, count })
            .collect();

        Self {
            total_nodes: nodes.len(),
            total_edges: edges.len(),
            languages,
            modules,
            types,
            entry_points,
            edge_types,
        }
    }
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
        let (paths, truncated) = trace_breadth_first(&origin, &graph, request);
        let result = TracePathResult {
            project: request.project.as_str().to_owned(),
            origin: node_summary(&origin, None, &graph.degrees, Vec::new()),
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

impl From<&TracePathRequest> for TraceCacheKey {
    fn from(request: &TracePathRequest) -> Self {
        Self {
            function_name: request.function_name.clone(),
            direction: match request.direction {
                TraceDirection::Inbound => 0,
                TraceDirection::Outbound => 1,
                TraceDirection::Both => 2,
            },
            depth: request.depth,
            limit: request.limit,
            edge_types: request.edge_types.clone(),
        }
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

pub(crate) fn degrees(edges: &[GraphEdge]) -> BTreeMap<NodeId, (usize, usize)> {
    let mut degrees = BTreeMap::new();
    for edge in edges {
        degrees.entry(edge.source.clone()).or_insert((0, 0)).1 += 1;
        degrees.entry(edge.target.clone()).or_insert((0, 0)).0 += 1;
    }
    degrees
}

pub(crate) fn node_summary(
    node: &GraphNode,
    rank: Option<f64>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    connected_names: Vec<String>,
) -> NodeSummary {
    let (in_degree, out_degree) = degrees.get(&node.id).copied().unwrap_or((0, 0));
    NodeSummary {
        id: node.id.as_str().to_owned(),
        name: node.name.clone(),
        qualified_name: node.qualified_name.as_str().to_owned(),
        label: node.label.as_str().to_owned(),
        file_path: node.file_path.as_ref().map(|path| path.as_str().to_owned()),
        start_byte: node.source_span.map(|span| span.bytes.start),
        end_byte: node.source_span.map(|span| span.bytes.end),
        start_line: node.source_span.map(|span| span.start.row + 1),
        end_line: node.source_span.map(|span| span.end.row + 1),
        generation: node.generation.value(),
        in_degree,
        out_degree,
        rank,
        connected_names,
        properties: node.properties.clone(),
    }
}

#[cfg(test)]
mod cache_tests {
    use std::{cell::Cell, sync::Arc};

    use goldeneye_domain::{Generation, ProjectId};

    use crate::types::{SearchGraphPage, SearchGraphRequest};

    use super::{ProjectGraph, QueryCache, SearchCacheKey};

    #[test]
    fn graph_cache_reuses_one_generation_and_reloads_the_next() {
        let cache = QueryCache::default();
        let project = ProjectId::new("demo").expect("project ID");
        let loads = Cell::new(0_u8);
        let mut load = || {
            loads.set(loads.get() + 1);
            Ok((Vec::new(), Vec::new()))
        };

        let first = cache
            .get_or_load(&project, Generation::new(1), &mut load)
            .expect("first graph load");
        let reused = cache
            .get_or_load(&project, Generation::new(1), &mut load)
            .expect("cached graph load");
        let replaced = cache
            .get_or_load(&project, Generation::new(2), &mut load)
            .expect("replacement graph load");
        let first_summary = first.architecture_summary();
        let reused_summary = reused.architecture_summary();
        let replaced_summary = replaced.architecture_summary();

        assert!(Arc::ptr_eq(&first, &reused));
        assert!(!Arc::ptr_eq(&first, &replaced));
        assert!(std::ptr::eq(first_summary, reused_summary));
        assert!(!std::ptr::eq(first_summary, replaced_summary));
        assert_eq!(loads.get(), 2);
    }

    #[test]
    fn graph_cache_invalidation_reloads_an_unchanged_generation() {
        let cache = QueryCache::default();
        let first_project = ProjectId::new("first").expect("project ID");
        let second_project = ProjectId::new("second").expect("project ID");
        let loads = Cell::new(0_u8);
        let mut load = || {
            loads.set(loads.get() + 1);
            Ok((Vec::new(), Vec::new()))
        };

        cache
            .get_or_load(&first_project, Generation::new(1), &mut load)
            .expect("first graph load");
        cache.invalidate_project(&first_project);
        cache
            .get_or_load(&first_project, Generation::new(1), &mut load)
            .expect("invalidated graph reload");
        cache
            .get_or_load(&second_project, Generation::new(1), &mut load)
            .expect("second graph load");
        cache.invalidate_all();
        cache
            .get_or_load(&first_project, Generation::new(1), &mut load)
            .expect("all-invalidated first reload");
        cache
            .get_or_load(&second_project, Generation::new(1), &mut load)
            .expect("all-invalidated second reload");

        assert_eq!(loads.get(), 5);
    }

    #[test]
    fn search_page_cache_is_scoped_to_one_project_graph_generation() {
        let graph = ProjectGraph::new(Generation::new(1), Vec::new(), Vec::new());
        let mut request = SearchGraphRequest::new(ProjectId::new("demo").expect("project ID"));
        request.query = Some("fs search".to_owned());
        let key = SearchCacheKey::from(&request);
        let page = SearchGraphPage {
            project: "demo".to_owned(),
            results: Vec::new(),
            total: 0,
            has_more: false,
            next_cursor: None,
        };

        assert_eq!(graph.cached_search(&key), None);
        graph.cache_search(key.clone(), page.clone());
        assert_eq!(graph.cached_search(&key), Some(page));

        let next_generation = ProjectGraph::new(Generation::new(2), Vec::new(), Vec::new());
        assert_eq!(next_generation.cached_search(&key), None);
    }
}
