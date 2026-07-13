use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use goldeneye_domain::{ContentHash, FileId, GraphEdge, GraphNode, NodeId, ProjectId};
use goldeneye_store::{QueryStore, SearchHit, Store};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::types::{
    ArchitectureModule, ArchitectureRequest, ArchitectureResult, CodeSnippetRequest,
    CodeSnippetResult, CountSummary, GraphSchemaRequest, GraphSchemaResult, IndexStatusRequest,
    IndexStatusResult, NodeSummary, ProjectSummary, QueryError, QueryGraphRequest,
    QueryGraphResult, SchemaEntry, SearchGraphPage, SearchGraphRequest, TraceDirection, TraceHop,
    TracePathRequest, TracePathResult,
};

const MAX_PAGE_SIZE: usize = 200;
const MAX_SEARCH_CANDIDATES: usize = 50_000;
const SEARCH_CHUNK: usize = 1_000;
const MAX_TRACE_DEPTH: usize = 5;
const MAX_TRACE_LIMIT: usize = 1_000;
const MAX_SNIPPET_BYTES: usize = 1_048_576;
const MAX_SNIPPET_LINES: usize = 10_000;

pub struct QueryEngine {
    store: QueryStore,
}

impl QueryEngine {
    /// Opens an existing graph database in read-only/query-only mode.
    ///
    /// # Errors
    ///
    /// Returns a query error when the database is missing or cannot be opened safely.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, QueryError> {
        Ok(Self {
            store: Store::open_read_only(path)?,
        })
    }

    #[must_use]
    pub const fn from_store(store: QueryStore) -> Self {
        Self { store }
    }

    /// Lists indexed projects in bytewise project-ID order.
    ///
    /// # Errors
    ///
    /// Returns a query error when the registry cannot be read.
    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>, QueryError> {
        self.store
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
        let counts = self.store.counts(&request.project)?;
        let settings = self.store.connection_settings()?;
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
        self.require_project(&request.project)?;
        let nodes = self.store.list_nodes(&request.project)?;
        let edges = self.store.list_edges(&request.project)?;
        let schema = self.store.schema_info()?;
        Ok(GraphSchemaResult {
            project: request.project.as_str().to_owned(),
            schema_version: schema.version,
            node_labels: schema_entries(
                nodes
                    .iter()
                    .map(|node| (node.label.as_str(), node.properties.keys())),
            ),
            edge_types: schema_entries(
                edges
                    .iter()
                    .map(|edge| (edge.kind.as_str(), edge.properties.keys())),
            ),
        })
    }

    /// Searches graph nodes with deterministic filtering and cursor pagination.
    ///
    /// # Errors
    ///
    /// Returns a query error for invalid filters/cursors, absent projects, or store failures.
    pub fn search_graph(
        &self,
        request: &SearchGraphRequest,
    ) -> Result<SearchGraphPage, QueryError> {
        self.require_project(&request.project)?;
        if request.page.limit == 0 || request.page.limit > MAX_PAGE_SIZE {
            return Err(QueryError::InvalidPageLimit {
                actual: request.page.limit,
                maximum: MAX_PAGE_SIZE,
            });
        }
        let fingerprint = search_fingerprint(request);
        let offset = page_offset(request, &fingerprint)?;
        let name = compile_pattern("name_pattern", request.name_pattern.as_deref())?;
        let qualified_name = compile_pattern(
            "qualified_name_pattern",
            request.qualified_name_pattern.as_deref(),
        )?;
        let file = compile_pattern("file_pattern", request.file_pattern.as_deref())?;

        let edges = self.store.list_edges(&request.project)?;
        let degrees = degrees(&edges);
        let mut candidates = self.search_candidates(request)?;
        candidates.retain(|candidate| {
            matches_search_filters(
                &candidate.node,
                request,
                name.as_ref(),
                qualified_name.as_ref(),
                file.as_ref(),
                &edges,
                &degrees,
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
            self.store
                .list_nodes(&request.project)?
                .into_iter()
                .map(|node| (node.id, node.name))
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
                    &edges,
                    &connected_nodes,
                );
                node_summary(&candidate.node, candidate.rank, &degrees, connected_names)
            })
            .collect();
        let has_more = end < total;
        Ok(SearchGraphPage {
            project: request.project.as_str().to_owned(),
            results,
            total,
            has_more,
            next_cursor: has_more.then(|| format_cursor(&fingerprint, end)),
        })
    }

    /// Traverses graph edges breadth-first with deterministic cycle suppression.
    ///
    /// # Errors
    ///
    /// Returns a query error for invalid bounds, ambiguous/missing symbols, or store failures.
    pub fn trace_path(&self, request: &TracePathRequest) -> Result<TracePathResult, QueryError> {
        self.require_project(&request.project)?;
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
        let nodes = self.store.list_nodes(&request.project)?;
        let edges = self.store.list_edges(&request.project)?;
        let degrees = degrees(&edges);
        let origin = resolve_symbol(
            &request.function_name,
            &nodes,
            &degrees,
            ResolveMode::Callable,
        )?;
        let (paths, truncated) = trace_breadth_first(&origin, &nodes, &edges, request);
        Ok(TracePathResult {
            project: request.project.as_str().to_owned(),
            origin: node_summary(&origin, None, &degrees, Vec::new()),
            direction: request.direction,
            paths,
            truncated,
        })
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
        validate_snippet_limit("max_bytes", request.max_bytes, MAX_SNIPPET_BYTES)?;
        validate_snippet_limit("max_lines", request.max_lines, MAX_SNIPPET_LINES)?;
        let nodes = self.store.list_nodes(&request.project)?;
        let edges = self.store.list_edges(&request.project)?;
        let degrees = degrees(&edges);
        let symbol = resolve_symbol(&request.qualified_name, &nodes, &degrees, ResolveMode::Any)?;
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
        let file = self
            .store
            .get_file(&FileId::new(request.project.clone(), file_path.clone()))?
            .ok_or_else(|| QueryError::IndexedFileNotFound {
                path: file_path.as_str().to_owned(),
            })?;
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
            symbol: node_summary(&symbol, None, &degrees, Vec::new()),
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
        let nodes = self.store.list_nodes(&request.project)?;
        let edges = self.store.list_edges(&request.project)?;
        let degrees = degrees(&edges);

        let mut languages: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for node in &nodes {
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
                defined_symbols: edges
                    .iter()
                    .filter(|edge| edge.source == node.id && edge.kind.as_str() == "DEFINES")
                    .count(),
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
            .map(|node| node_summary(node, None, &degrees, Vec::new()))
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
            .map(|node| node_summary(node, None, &degrees, Vec::new()))
            .take(20)
            .collect();
        let mut edge_counts: BTreeMap<String, u64> = BTreeMap::new();
        for edge in &edges {
            *edge_counts
                .entry(edge.kind.as_str().to_owned())
                .or_default() += 1;
        }
        let edge_types = edge_counts
            .into_iter()
            .map(|(name, count)| CountSummary { name, count })
            .collect();

        Ok(ArchitectureResult {
            project: project.id.as_str().to_owned(),
            root_path: project.root_path,
            generation: project.generation.value(),
            total_nodes: nodes.len(),
            total_edges: edges.len(),
            languages,
            modules,
            types,
            entry_points,
            edge_types,
        })
    }

    /// Executes the supported read-only Cypher subset over one project's graph.
    ///
    /// # Errors
    ///
    /// Returns a query error for mutation attempts, unsupported/syntax-invalid queries, absent
    /// projects, invalid row bounds, or store failures.
    pub fn query_graph(&self, request: &QueryGraphRequest) -> Result<QueryGraphResult, QueryError> {
        self.require_project(&request.project)?;
        let nodes = self.store.list_nodes(&request.project)?;
        let edges = self.store.list_edges(&request.project)?;
        crate::cypher::execute(request, &nodes, &edges)
    }

    fn require_project(
        &self,
        project: &ProjectId,
    ) -> Result<goldeneye_domain::ProjectRecord, QueryError> {
        self.store
            .get_project(project)?
            .ok_or_else(|| QueryError::ProjectNotFound(project.clone()))
    }

    fn search_candidates(
        &self,
        request: &SearchGraphRequest,
    ) -> Result<Vec<Candidate>, QueryError> {
        let Some(query) = request.query.as_deref().filter(|query| !query.is_empty()) else {
            return Ok(self
                .store
                .list_nodes(&request.project)?
                .into_iter()
                .map(|node| Candidate { node, rank: None })
                .collect());
        };
        let total = self.store.count_search_nodes(&request.project, query)?;
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
                self.store
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

fn degrees(edges: &[GraphEdge]) -> BTreeMap<NodeId, (usize, usize)> {
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
enum ResolveMode {
    Any,
    Callable,
}

fn resolve_symbol(
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
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    request: &TracePathRequest,
) -> (Vec<TraceHop>, bool) {
    let nodes_by_id: BTreeMap<&NodeId, &GraphNode> =
        nodes.iter().map(|node| (&node.id, node)).collect();
    let edge_types: BTreeSet<&str> = request.edge_types.iter().map(String::as_str).collect();
    let mut visited = BTreeSet::from([origin.id.clone()]);
    let mut frontier = vec![origin.id.clone()];
    let mut paths = Vec::new();
    let mut truncated = false;

    'depth: for hop in 1..=request.depth {
        let mut candidates = Vec::new();
        for current in &frontier {
            for edge in edges
                .iter()
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
            node_qualified_name(&nodes_by_id, left_id)
                .cmp(node_qualified_name(&nodes_by_id, right_id))
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
            let Some(source) = nodes_by_id.get(&edge.source) else {
                continue;
            };
            let Some(target) = nodes_by_id.get(&edge.target) else {
                continue;
            };
            let Some(related) = nodes_by_id.get(&related_id) else {
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

fn node_qualified_name<'a>(nodes: &'a BTreeMap<&NodeId, &GraphNode>, node: &NodeId) -> &'a str {
    nodes
        .get(node)
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
