use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, OnceLock};

use goldeneye_domain::{
    ContentHash, FileId, FileRecord, Generation, GraphEdge, GraphNode, NodeId, ProjectId,
};
use goldeneye_ports::{QueryRepository, SearchHit};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::types::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, GraphSchemaRequest, GraphSchemaResult, IndexStatusRequest,
    IndexStatusResult, NodeSummary, ProjectSummary, QueryError, QueryGraphRequest,
    QueryGraphResult, SchemaEntry, SearchCodeRequest, SearchCodeResult, SearchGraphPage,
    SearchGraphRequest, SemanticSearchRequest, SemanticSearchResult, SimilaritySearchRequest,
    SimilaritySearchResult, TraceDirection, TraceHop, TracePathRequest, TracePathResult,
};

const MAX_PAGE_SIZE: usize = 200;
const MAX_SEARCH_CANDIDATES: usize = 50_000;
const SEARCH_CHUNK: usize = 1_000;
const MAX_TRACE_DEPTH: usize = 5;
const MAX_TRACE_LIMIT: usize = 1_000;
const MAX_SNIPPET_BYTES: usize = 1_048_576;
const MAX_SNIPPET_LINES: usize = 10_000;
const MAX_CACHED_SEARCH_PAGES: usize = 16;
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
struct SearchCacheKey {
    query: Option<String>,
    name_pattern: Option<String>,
    qualified_name_pattern: Option<String>,
    label: Option<String>,
    file_pattern: Option<String>,
    relationship: Option<String>,
    min_degree: Option<usize>,
    max_degree: Option<usize>,
    exclude_entry_points: bool,
    include_connected: bool,
    limit: usize,
    offset: usize,
    cursor: Option<String>,
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
        if request.page.limit == 0 || request.page.limit > MAX_PAGE_SIZE {
            return Err(QueryError::InvalidPageLimit {
                actual: request.page.limit,
                maximum: MAX_PAGE_SIZE,
            });
        }
        let fingerprint = search_fingerprint(request);
        let offset = page_offset(request, &fingerprint)?;
        let cache_key = SearchCacheKey::from(request);
        if let Some(page) = graph.cached_search(&cache_key) {
            return Ok(page);
        }
        let name = compile_pattern("name_pattern", request.name_pattern.as_deref())?;
        let qualified_name = compile_pattern(
            "qualified_name_pattern",
            request.qualified_name_pattern.as_deref(),
        )?;
        let file = compile_pattern("file_pattern", request.file_pattern.as_deref())?;

        let mut candidates = self.search_candidates(request, &graph)?;
        candidates.retain(|candidate| {
            matches_search_filters(
                &candidate.node,
                request,
                name.as_ref(),
                qualified_name.as_ref(),
                file.as_ref(),
                &graph.edges,
                &graph.degrees,
            )
        });
        candidates.sort_by(|left, right| {
            rank_cmp(left.rank, right.rank)
                .then_with(|| left.node.qualified_name.cmp(&right.node.qualified_name))
                .then_with(|| left.node.id.cmp(&right.node.id))
        });

        let total = candidates.len();
        let end = offset.saturating_add(request.page.limit).min(total);
        let connected_nodes = if request.include_connected {
            graph
                .nodes
                .iter()
                .map(|node| (node.id.clone(), node.name.clone()))
                .collect()
        } else {
            BTreeMap::new()
        };
        let results = candidates
            .get(offset..end)
            .unwrap_or_default()
            .iter()
            .map(|candidate| {
                let connected_names = connected_names(
                    &candidate.node,
                    request.relationship.as_deref().unwrap_or("CALLS"),
                    &graph.edges,
                    &connected_nodes,
                );
                node_summary(
                    &candidate.node,
                    candidate.rank,
                    &graph.degrees,
                    connected_names,
                )
            })
            .collect();
        let has_more = end < total;
        let page = SearchGraphPage {
            project: request.project.as_str().to_owned(),
            results,
            total,
            has_more,
            next_cursor: has_more.then(|| format_cursor(&fingerprint, end)),
        };
        graph.cache_search(cache_key, page.clone());
        Ok(page)
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
        validate_snippet_limit("max_bytes", request.max_bytes, MAX_SNIPPET_BYTES)?;
        validate_snippet_limit("max_lines", request.max_lines, MAX_SNIPPET_LINES)?;
        let symbol = resolve_symbol_in_graph(&request.qualified_name, &graph, ResolveMode::Any)?;
        let file_path =
            symbol
                .file_path
                .clone()
                .ok_or_else(|| QueryError::SourceFileUnavailable {
                    qualified_name: symbol.qualified_name.as_str().to_owned(),
                })?;
        let span = symbol
            .source_span
            .ok_or_else(|| QueryError::SourceSpanUnavailable {
                qualified_name: symbol.qualified_name.as_str().to_owned(),
            })?;
        let file = if let Some(file) = graph.cached_file(file_path.as_str()) {
            file
        } else {
            let file = self
                .repository
                .get_file(&FileId::new(request.project.clone(), file_path.clone()))?
                .ok_or_else(|| QueryError::IndexedFileNotFound {
                    path: file_path.as_str().to_owned(),
                })?;
            graph.cache_file(file.clone());
            file
        };
        let absolute_path = std::path::Path::new(&project.root_path).join(file_path.as_str());
        let bytes = std::fs::read(&absolute_path).map_err(|source| QueryError::SourceRead {
            path: absolute_path,
            source,
        })?;
        let actual_hash = ContentHash::of(&bytes);
        if actual_hash != file.content_hash {
            return Err(QueryError::StaleFile {
                path: file_path.as_str().to_owned(),
                expected_hash: hash_hex(&file.content_hash),
                actual_hash: hash_hex(&actual_hash),
            });
        }
        let start =
            usize::try_from(span.bytes.start).map_err(|_| QueryError::CorruptSourceSpan {
                qualified_name: symbol.qualified_name.as_str().to_owned(),
            })?;
        let end = usize::try_from(span.bytes.end).map_err(|_| QueryError::CorruptSourceSpan {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
        })?;
        let source_bytes = bytes
            .get(start..end)
            .ok_or_else(|| QueryError::CorruptSourceSpan {
                qualified_name: symbol.qualified_name.as_str().to_owned(),
            })?;
        let line_count = source_line_count(source_bytes);
        if source_bytes.len() > request.max_bytes || line_count > request.max_lines {
            return Err(QueryError::SnippetTooLarge {
                actual_bytes: source_bytes.len(),
                actual_lines: line_count,
                maximum_bytes: request.max_bytes,
                maximum_lines: request.max_lines,
            });
        }
        let source =
            String::from_utf8(source_bytes.to_vec()).map_err(|_| QueryError::SourceNotUtf8 {
                qualified_name: symbol.qualified_name.as_str().to_owned(),
            })?;
        let start_line = span.start.row + 1;
        let end_line = start_line + u64::try_from(line_count.saturating_sub(1)).unwrap_or(u64::MAX);
        Ok(CodeSnippetResult {
            project: request.project.as_str().to_owned(),
            symbol: node_summary(&symbol, None, &graph.degrees, Vec::new()),
            source,
            file_path: file_path.as_str().to_owned(),
            start_byte: start,
            end_byte: end,
            start_line,
            end_line,
            content_hash: hash_hex(&file.content_hash),
        })
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

    fn search_candidates(
        &self,
        request: &SearchGraphRequest,
        graph: &ProjectGraph,
    ) -> Result<Vec<Candidate>, QueryError> {
        let Some(query) = request.query.as_deref().filter(|query| !query.is_empty()) else {
            if let Some(name) = request
                .name_pattern
                .as_deref()
                .and_then(exact_pattern_literal)
            {
                return Ok(graph
                    .nodes_by_name
                    .get(name)
                    .into_iter()
                    .flatten()
                    .filter_map(|index| graph.nodes.get(*index))
                    .cloned()
                    .map(|node| Candidate { node, rank: None })
                    .collect());
            }
            return Ok(graph
                .nodes
                .iter()
                .cloned()
                .map(|node| Candidate { node, rank: None })
                .collect());
        };
        let total = self
            .repository
            .count_search_nodes(&request.project, query)?;
        if total > u64::try_from(MAX_SEARCH_CANDIDATES).expect("constant fits u64") {
            return Err(QueryError::TooManySearchCandidates {
                actual: total,
                maximum: MAX_SEARCH_CANDIDATES,
            });
        }
        let total = usize::try_from(total).expect("bounded search count fits usize");
        let mut candidates = Vec::with_capacity(total);
        for offset in (0..total).step_by(SEARCH_CHUNK) {
            let limit = SEARCH_CHUNK.min(total - offset);
            candidates.extend(
                self.repository
                    .search_nodes_page(&request.project, query, limit, offset)?
                    .into_iter()
                    .map(Candidate::from),
            );
        }
        Ok(candidates)
    }
}

struct Candidate {
    node: GraphNode,
    rank: Option<f64>,
}

impl From<&SearchGraphRequest> for SearchCacheKey {
    fn from(request: &SearchGraphRequest) -> Self {
        Self {
            query: request.query.clone(),
            name_pattern: request.name_pattern.clone(),
            qualified_name_pattern: request.qualified_name_pattern.clone(),
            label: request.label.clone(),
            file_pattern: request.file_pattern.clone(),
            relationship: request.relationship.clone(),
            min_degree: request.min_degree,
            max_degree: request.max_degree,
            exclude_entry_points: request.exclude_entry_points,
            include_connected: request.include_connected,
            limit: request.page.limit,
            offset: request.page.offset,
            cursor: request.page.cursor.clone(),
        }
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

impl From<SearchHit> for Candidate {
    fn from(hit: SearchHit) -> Self {
        Self {
            node: hit.node,
            rank: Some(hit.rank),
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

fn compile_pattern(
    field: &'static str,
    pattern: Option<&str>,
) -> Result<Option<Regex>, QueryError> {
    pattern
        .map(|pattern| {
            Regex::new(pattern).map_err(|source| QueryError::InvalidPattern { field, source })
        })
        .transpose()
}

fn exact_pattern_literal(pattern: &str) -> Option<&str> {
    let literal = pattern.strip_prefix('^')?.strip_suffix('$')?;
    (!literal.is_empty()
        && literal
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'))
    .then_some(literal)
}

pub(crate) fn degrees(edges: &[GraphEdge]) -> BTreeMap<NodeId, (usize, usize)> {
    let mut degrees = BTreeMap::new();
    for edge in edges {
        degrees.entry(edge.source.clone()).or_insert((0, 0)).1 += 1;
        degrees.entry(edge.target.clone()).or_insert((0, 0)).0 += 1;
    }
    degrees
}

#[allow(clippy::too_many_arguments)]
fn matches_search_filters(
    node: &GraphNode,
    request: &SearchGraphRequest,
    name: Option<&Regex>,
    qualified_name: Option<&Regex>,
    file: Option<&Regex>,
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> bool {
    if request
        .label
        .as_deref()
        .is_some_and(|label| node.label.as_str() != label)
        || name.is_some_and(|pattern| !pattern.is_match(&node.name))
        || qualified_name.is_some_and(|pattern| !pattern.is_match(node.qualified_name.as_str()))
        || file.is_some_and(|pattern| {
            !node
                .file_path
                .as_ref()
                .is_some_and(|path| pattern.is_match(path.as_str()))
        })
    {
        return false;
    }
    let (in_degree, out_degree) = degrees.get(&node.id).copied().unwrap_or((0, 0));
    let degree = in_degree + out_degree;
    if request.min_degree.is_some_and(|minimum| degree < minimum)
        || request.max_degree.is_some_and(|maximum| degree > maximum)
    {
        return false;
    }
    if request.relationship.as_deref().is_some_and(|kind| {
        !edges.iter().any(|edge| {
            edge.kind.as_str() == kind && (edge.source == node.id || edge.target == node.id)
        })
    }) {
        return false;
    }
    !request.exclude_entry_points
        || !node
            .properties
            .get("is_entry_point")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
}

fn rank_cmp(left: Option<f64>, right: Option<f64>) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.total_cmp(&right),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
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

fn connected_names(
    node: &GraphNode,
    relationship: &str,
    edges: &[GraphEdge],
    names: &BTreeMap<NodeId, String>,
) -> Vec<String> {
    let mut connected = BTreeSet::new();
    for edge in edges
        .iter()
        .filter(|edge| edge.kind.as_str() == relationship)
    {
        let related = if edge.source == node.id {
            Some(&edge.target)
        } else if edge.target == node.id {
            Some(&edge.source)
        } else {
            None
        };
        if let Some(name) = related.and_then(|id| names.get(id)) {
            connected.insert(name.clone());
        }
    }
    connected.into_iter().collect()
}

fn search_fingerprint(request: &SearchGraphRequest) -> String {
    let values = [
        Some(request.project.as_str()),
        request.query.as_deref(),
        request.name_pattern.as_deref(),
        request.qualified_name_pattern.as_deref(),
        request.label.as_deref(),
        request.file_pattern.as_deref(),
        request.relationship.as_deref(),
    ];
    let mut hash = Sha256::new();
    for value in values {
        hash.update(value.unwrap_or_default().as_bytes());
        hash.update([0]);
    }
    for value in [request.min_degree, request.max_degree] {
        hash.update([u8::from(value.is_some())]);
        hash.update(value.unwrap_or_default().to_le_bytes());
    }
    hash.update([
        u8::from(request.exclude_entry_points),
        u8::from(request.include_connected),
    ]);
    let mut fingerprint = String::with_capacity(16);
    for byte in &hash.finalize()[..8] {
        write!(&mut fingerprint, "{byte:02x}").expect("writing to String cannot fail");
    }
    fingerprint
}

fn page_offset(request: &SearchGraphRequest, fingerprint: &str) -> Result<usize, QueryError> {
    let Some(cursor) = request.page.cursor.as_deref() else {
        return Ok(request.page.offset);
    };
    if request.page.offset != 0 {
        return Err(QueryError::CursorWithOffset);
    }
    let mut parts = cursor.split(':');
    if parts.next() != Some("geq1") {
        return Err(QueryError::InvalidCursor);
    }
    if parts.next() != Some(fingerprint) {
        return Err(QueryError::CursorMismatch);
    }
    let offset = parts
        .next()
        .ok_or(QueryError::InvalidCursor)?
        .parse()
        .map_err(|_| QueryError::InvalidCursor)?;
    if parts.next().is_some() {
        return Err(QueryError::InvalidCursor);
    }
    Ok(offset)
}

fn format_cursor(fingerprint: &str, offset: usize) -> String {
    format!("geq1:{fingerprint}:{offset}")
}

#[derive(Clone, Copy)]
pub(crate) enum ResolveMode {
    Any,
    Callable,
}

fn resolve_symbol_in_graph(
    query: &str,
    graph: &ProjectGraph,
    mode: ResolveMode,
) -> Result<GraphNode, QueryError> {
    if let Some(node) = graph
        .nodes_by_qualified_name
        .get(query)
        .and_then(|index| graph.nodes.get(*index))
        .filter(|node| resolve_eligible(node, mode))
    {
        return Ok(node.clone());
    }
    resolve_symbol(query, &graph.nodes, &graph.degrees, mode)
}

pub(crate) fn resolve_symbol(
    query: &str,
    nodes: &[GraphNode],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    mode: ResolveMode,
) -> Result<GraphNode, QueryError> {
    let eligible = |node: &&GraphNode| resolve_eligible(node, mode);
    if let Some(node) = nodes
        .iter()
        .filter(eligible)
        .find(|node| node.qualified_name.as_str() == query)
    {
        return Ok(node.clone());
    }

    let mut candidates: Vec<&GraphNode> = if is_qualified_fragment(query) {
        nodes
            .iter()
            .filter(eligible)
            .filter(|node| qualified_suffix_matches(node.qualified_name.as_str(), query))
            .collect()
    } else {
        nodes
            .iter()
            .filter(eligible)
            .filter(|node| node.name == query)
            .collect()
    };
    candidates.sort_by(|left, right| {
        left.qualified_name
            .cmp(&right.qualified_name)
            .then_with(|| left.id.cmp(&right.id))
    });
    match candidates.as_slice() {
        [node] => Ok((*node).clone()),
        [] => Err(QueryError::SymbolNotFound {
            query: query.to_owned(),
            suggestions: symbol_suggestions(query, nodes, degrees, mode),
        }),
        _ => Err(QueryError::AmbiguousSymbol {
            query: query.to_owned(),
            candidates: candidates
                .into_iter()
                .map(|node| node_summary(node, None, degrees, Vec::new()))
                .collect(),
        }),
    }
}

fn is_qualified_fragment(query: &str) -> bool {
    query.contains('.') || query.contains("::") || query.contains('/')
}

fn qualified_suffix_matches(qualified_name: &str, query: &str) -> bool {
    qualified_name == query
        || qualified_name.strip_suffix(query).is_some_and(|prefix| {
            prefix.ends_with('.') || prefix.ends_with("::") || prefix.ends_with('/')
        })
}

fn symbol_suggestions(
    query: &str,
    nodes: &[GraphNode],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    mode: ResolveMode,
) -> Vec<NodeSummary> {
    let folded = query.to_lowercase();
    let mut matches: Vec<&GraphNode> = nodes
        .iter()
        .filter(|node| resolve_eligible(node, mode))
        .filter(|node| {
            node.name.to_lowercase().contains(&folded)
                || node
                    .qualified_name
                    .as_str()
                    .to_lowercase()
                    .contains(&folded)
        })
        .collect();
    matches.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
    matches
        .into_iter()
        .take(10)
        .map(|node| node_summary(node, None, degrees, Vec::new()))
        .collect()
}

fn resolve_eligible(node: &GraphNode, mode: ResolveMode) -> bool {
    match mode {
        ResolveMode::Any => true,
        ResolveMode::Callable => matches!(node.label.as_str(), "Function" | "Method"),
    }
}

fn trace_breadth_first(
    origin: &GraphNode,
    graph: &ProjectGraph,
    request: &TracePathRequest,
) -> (Vec<TraceHop>, bool) {
    let nodes = &graph.nodes;
    let edges = &graph.edges;
    let edge_types: BTreeSet<&str> = request.edge_types.iter().map(String::as_str).collect();
    let mut visited = BTreeSet::from([origin.id.clone()]);
    let mut frontier = vec![origin.id.clone()];
    let mut paths = Vec::new();
    let mut truncated = false;

    'depth: for hop in 1..=request.depth {
        let mut candidates = Vec::new();
        for current in &frontier {
            for edge in graph
                .edges_by_node
                .get(current)
                .into_iter()
                .flatten()
                .filter_map(|index| edges.get(*index))
                .filter(|edge| edge_types.contains(edge.kind.as_str()))
            {
                let related = match request.direction {
                    TraceDirection::Outbound | TraceDirection::Both if edge.source == *current => {
                        Some(&edge.target)
                    }
                    TraceDirection::Inbound | TraceDirection::Both if edge.target == *current => {
                        Some(&edge.source)
                    }
                    _ => None,
                };
                if let Some(related) = related.filter(|related| !visited.contains(*related)) {
                    candidates.push((related.clone(), edge));
                }
            }
        }
        candidates.sort_by(|(left_id, left_edge), (right_id, right_edge)| {
            node_qualified_name(graph, left_id)
                .cmp(node_qualified_name(graph, right_id))
                .then_with(|| left_edge.source.cmp(&right_edge.source))
                .then_with(|| left_edge.target.cmp(&right_edge.target))
                .then_with(|| left_edge.kind.cmp(&right_edge.kind))
        });
        let mut next = Vec::new();
        for (related_id, edge) in candidates {
            if !visited.insert(related_id.clone()) {
                continue;
            }
            if paths.len() == request.limit {
                truncated = true;
                break 'depth;
            }
            let Some(source) = graph
                .nodes_by_id
                .get(&edge.source)
                .and_then(|index| nodes.get(*index))
            else {
                continue;
            };
            let Some(target) = graph
                .nodes_by_id
                .get(&edge.target)
                .and_then(|index| nodes.get(*index))
            else {
                continue;
            };
            let Some(related) = graph
                .nodes_by_id
                .get(&related_id)
                .and_then(|index| nodes.get(*index))
            else {
                continue;
            };
            paths.push(TraceHop {
                source_qualified_name: source.qualified_name.as_str().to_owned(),
                target_qualified_name: target.qualified_name.as_str().to_owned(),
                related_qualified_name: related.qualified_name.as_str().to_owned(),
                edge_kind: edge.kind.as_str().to_owned(),
                hop,
                file_path: related
                    .file_path
                    .as_ref()
                    .map(|path| path.as_str().to_owned()),
                line: related.source_span.map(|span| span.start.row + 1),
            });
            next.push(related_id);
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    (paths, truncated)
}

fn node_qualified_name<'a>(graph: &'a ProjectGraph, node: &NodeId) -> &'a str {
    graph
        .nodes_by_id
        .get(node)
        .and_then(|index| graph.nodes.get(*index))
        .map_or("", |node| node.qualified_name.as_str())
}

fn validate_snippet_limit(
    field: &'static str,
    actual: usize,
    maximum: usize,
) -> Result<(), QueryError> {
    if actual == 0 || actual > maximum {
        return Err(QueryError::InvalidSnippetLimit {
            field,
            actual,
            maximum,
        });
    }
    Ok(())
}

fn source_line_count(source: &[u8]) -> usize {
    if source.is_empty() {
        return 0;
    }
    source.split(|byte| *byte == b'\n').count() - usize::from(source.ends_with(b"\n"))
}

fn hash_hex(hash: &ContentHash) -> String {
    let mut encoded = String::with_capacity(hash.as_bytes().len() * 2);
    for byte in hash.as_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod cache_tests {
    use std::{cell::Cell, sync::Arc};

    use goldeneye_domain::{Generation, ProjectId};

    use crate::types::SearchGraphPage;

    use super::{ProjectGraph, QueryCache, SearchCacheKey, exact_pattern_literal};

    #[test]
    fn exact_pattern_literal_only_accepts_anchored_identifier_names() {
        assert_eq!(exact_pattern_literal("^fs_search$"), Some("fs_search"));
        assert_eq!(exact_pattern_literal("fs_search"), None);
        assert_eq!(exact_pattern_literal("^fs_.*$"), None);
    }

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
        let key = SearchCacheKey {
            query: Some("fs search".to_owned()),
            name_pattern: None,
            qualified_name_pattern: None,
            label: None,
            file_pattern: None,
            relationship: None,
            min_degree: None,
            max_degree: None,
            exclude_entry_points: false,
            include_connected: false,
            limit: 20,
            offset: 0,
            cursor: None,
        };
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
