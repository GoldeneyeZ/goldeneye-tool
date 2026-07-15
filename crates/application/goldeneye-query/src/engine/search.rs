use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};
use goldeneye_ports::{QueryRepository, SearchHit};
use regex::Regex;
use sha2::{Digest, Sha256};

use crate::types::{QueryError, SearchGraphPage, SearchGraphRequest};

use super::{ProjectGraph, node_summary};

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

pub(super) fn execute(
    repository: &dyn QueryRepository,
    request: &SearchGraphRequest,
    graph: &ProjectGraph,
) -> Result<SearchGraphPage, QueryError> {
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

    let mut candidates = search_candidates(repository, request, graph)?;
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

fn search_candidates(
    repository: &dyn QueryRepository,
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
