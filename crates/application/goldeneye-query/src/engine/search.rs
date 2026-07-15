mod pagination;

use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};
use goldeneye_ports::{QueryRepository, SearchHit};
use regex::Regex;

use crate::types::{QueryError, SearchGraphPage, SearchGraphRequest};

use super::{ProjectGraph, node_summary};
use pagination::{format_cursor, page_offset, search_fingerprint};

const MAX_PAGE_SIZE: usize = 200;
const MAX_SEARCH_CANDIDATES: usize = 50_000;
const SEARCH_CHUNK: usize = 1_000;
pub(super) const MAX_CACHED_SEARCH_PAGES: usize = 16;

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct SearchCacheKey {
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

struct Candidate {
    node: GraphNode,
    rank: Option<f64>,
}

struct CompiledPatterns {
    name: Option<Regex>,
    qualified_name: Option<Regex>,
    file: Option<Regex>,
}

pub(super) fn execute(
    repository: &dyn QueryRepository,
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
) -> Result<SearchGraphPage, QueryError> {
    validate_page_limit(request.page.limit)?;
    let fingerprint = search_fingerprint(request);
    let offset = page_offset(request, &fingerprint)?;
    let cache_key = SearchCacheKey::from(request);
    if let Some(page) = graph.cached_search(&cache_key) {
        return Ok(page);
    }
    let patterns = CompiledPatterns::new(request)?;
    let candidates = filtered_candidates(repository, request, graph, &patterns)?;
    let page = search_page(request, graph, &candidates, &fingerprint, offset);
    graph.cache_search(cache_key, page.clone());
    Ok(page)
}

impl CompiledPatterns {
    fn new(request: &SearchGraphRequest) -> Result<Self, QueryError> {
        Ok(Self {
            name: compile_pattern("name_pattern", request.name_pattern.as_deref())?,
            qualified_name: compile_pattern(
                "qualified_name_pattern",
                request.qualified_name_pattern.as_deref(),
            )?,
            file: compile_pattern("file_pattern", request.file_pattern.as_deref())?,
        })
    }
}

fn validate_page_limit(limit: usize) -> Result<(), QueryError> {
    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(QueryError::InvalidPageLimit {
            actual: limit,
            maximum: MAX_PAGE_SIZE,
        });
    }
    Ok(())
}

fn filtered_candidates(
    repository: &dyn QueryRepository,
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
    patterns: &CompiledPatterns,
) -> Result<Vec<Candidate>, QueryError> {
    let mut candidates = search_candidates(repository, request, graph)?;
    candidates
        .retain(|candidate| matches_search_filters(&candidate.node, request, graph, patterns));
    candidates.sort_by(|left, right| {
        rank_cmp(left.rank, right.rank)
            .then_with(|| left.node.qualified_name.cmp(&right.node.qualified_name))
            .then_with(|| left.node.id.cmp(&right.node.id))
    });
    Ok(candidates)
}

fn search_page(
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
    candidates: &[Candidate],
    fingerprint: &str,
    offset: usize,
) -> SearchGraphPage {
    let total = candidates.len();
    let end = offset.saturating_add(request.page.limit).min(total);
    let connected_nodes = connected_node_index(request, graph);
    let results = page_results(request, graph, candidates, offset, end, &connected_nodes);
    let has_more = end < total;
    SearchGraphPage {
        project: request.project.as_str().to_owned(),
        results,
        total,
        has_more,
        next_cursor: has_more.then(|| format_cursor(fingerprint, end)),
    }
}

fn connected_node_index(
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
) -> BTreeMap<NodeId, String> {
    if !request.include_connected {
        return BTreeMap::new();
    }
    graph
        .nodes
        .iter()
        .map(|node| (node.id.clone(), node.name.clone()))
        .collect()
}

fn page_results(
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
    candidates: &[Candidate],
    offset: usize,
    end: usize,
    connected_nodes: &BTreeMap<NodeId, String>,
) -> Vec<crate::types::NodeSummary> {
    candidates
        .get(offset..end)
        .unwrap_or_default()
        .iter()
        .map(|candidate| {
            let connected_names = connected_names(
                &candidate.node,
                request.relationship.as_deref().unwrap_or("CALLS"),
                &graph.edges,
                connected_nodes,
            );
            node_summary(
                &candidate.node,
                candidate.rank,
                &graph.degrees,
                connected_names,
            )
        })
        .collect()
}

fn search_candidates(
    repository: &dyn QueryRepository,
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
) -> Result<Vec<Candidate>, QueryError> {
    let Some(query) = request.query.as_deref().filter(|query| !query.is_empty()) else {
        return Ok(graph_candidates(request, graph));
    };
    repository_candidates(repository, request, query)
}

fn graph_candidates(request: &SearchGraphRequest, graph: &ProjectGraph) -> Vec<Candidate> {
    if let Some(name) = request
        .name_pattern
        .as_deref()
        .and_then(exact_pattern_literal)
    {
        return graph
            .nodes_by_name
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|index| graph.nodes.get(*index))
            .cloned()
            .map(|node| Candidate { node, rank: None })
            .collect();
    }
    graph
        .nodes
        .iter()
        .cloned()
        .map(|node| Candidate { node, rank: None })
        .collect()
}

fn repository_candidates(
    repository: &dyn QueryRepository,
    request: &SearchGraphRequest,
    query: &str,
) -> Result<Vec<Candidate>, QueryError> {
    let total = repository.count_search_nodes(&request.project, query)?;
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
            repository
                .search_nodes_page(&request.project, query, limit, offset)?
                .into_iter()
                .map(Candidate::from),
        );
    }
    Ok(candidates)
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

impl From<SearchHit> for Candidate {
    fn from(hit: SearchHit) -> Self {
        Self {
            node: hit.node,
            rank: Some(hit.rank),
        }
    }
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

fn matches_search_filters(
    node: &GraphNode,
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
    patterns: &CompiledPatterns,
) -> bool {
    matches_patterns(node, request, patterns)
        && matches_degree(node, request, &graph.degrees)
        && matches_relationship(node, request, &graph.edges)
        && matches_entry_point(node, request)
}

fn matches_patterns(
    node: &GraphNode,
    request: &SearchGraphRequest,
    patterns: &CompiledPatterns,
) -> bool {
    if request
        .label
        .as_deref()
        .is_some_and(|label| node.label.as_str() != label)
        || patterns
            .name
            .as_ref()
            .is_some_and(|pattern| !pattern.is_match(&node.name))
        || patterns
            .qualified_name
            .as_ref()
            .is_some_and(|pattern| !pattern.is_match(node.qualified_name.as_str()))
        || patterns.file.as_ref().is_some_and(|pattern| {
            !node
                .file_path
                .as_ref()
                .is_some_and(|path| pattern.is_match(path.as_str()))
        })
    {
        return false;
    }
    true
}

fn matches_degree(
    node: &GraphNode,
    request: &SearchGraphRequest,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> bool {
    let (in_degree, out_degree) = degrees.get(&node.id).copied().unwrap_or_default();
    let degree = in_degree + out_degree;
    request.min_degree.is_none_or(|minimum| degree >= minimum)
        && request.max_degree.is_none_or(|maximum| degree <= maximum)
}

fn matches_relationship(
    node: &GraphNode,
    request: &SearchGraphRequest,
    edges: &[GraphEdge],
) -> bool {
    !request.relationship.as_deref().is_some_and(|kind| {
        !edges.iter().any(|edge| {
            edge.kind.as_str() == kind && (edge.source == node.id || edge.target == node.id)
        })
    })
}

fn matches_entry_point(node: &GraphNode, request: &SearchGraphRequest) -> bool {
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

#[cfg(test)]
mod tests {
    use super::exact_pattern_literal;

    #[test]
    fn exact_pattern_literal_only_accepts_anchored_identifier_names() {
        assert_eq!(exact_pattern_literal("^fs_search$"), Some("fs_search"));
        assert_eq!(exact_pattern_literal("fs_search"), None);
        assert_eq!(exact_pattern_literal("^fs_.*$"), None);
    }
}
