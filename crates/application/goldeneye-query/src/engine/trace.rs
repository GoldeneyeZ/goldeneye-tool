use std::collections::BTreeSet;

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};

use super::ProjectGraph;
use crate::types::{TraceDirection, TraceHop, TracePathRequest};

pub(super) const MAX_CACHED_TRACE_RESULTS: usize = 16;

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct TraceCacheKey {
    function_name: String,
    direction: u8,
    depth: usize,
    limit: usize,
    edge_types: Vec<String>,
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

pub(super) fn trace_breadth_first(
    origin: &GraphNode,
    graph: &ProjectGraph,
    request: &TracePathRequest,
) -> (Vec<TraceHop>, bool) {
    let edge_types: BTreeSet<&str> = request.edge_types.iter().map(String::as_str).collect();
    let mut visited = BTreeSet::from([origin.id.clone()]);
    let mut frontier = vec![origin.id.clone()];
    let mut paths = Vec::new();

    for hop in 1..=request.depth {
        let mut candidates =
            trace_candidates(graph, &frontier, &visited, &edge_types, request.direction);
        sort_candidates(graph, &mut candidates);
        let mut next = Vec::new();
        for (related_id, edge) in candidates {
            if !visited.insert(related_id.clone()) {
                continue;
            }
            if paths.len() == request.limit {
                return (paths, true);
            }
            if let Some(path) = trace_hop(graph, &related_id, edge, hop) {
                paths.push(path);
                next.push(related_id);
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    (paths, false)
}

fn trace_candidates<'a>(
    graph: &'a ProjectGraph,
    frontier: &[NodeId],
    visited: &BTreeSet<NodeId>,
    edge_types: &BTreeSet<&str>,
    direction: TraceDirection,
) -> Vec<(NodeId, &'a GraphEdge)> {
    let mut candidates = Vec::new();
    for current in frontier {
        for edge in graph
            .edges_by_node
            .get(current)
            .into_iter()
            .flatten()
            .filter_map(|index| graph.edges.get(*index))
            .filter(|edge| edge_types.contains(edge.kind.as_str()))
        {
            if let Some(related) =
                related_node(edge, current, direction).filter(|related| !visited.contains(*related))
            {
                candidates.push((related.clone(), edge));
            }
        }
    }
    candidates
}

fn related_node<'a>(
    edge: &'a GraphEdge,
    current: &NodeId,
    direction: TraceDirection,
) -> Option<&'a NodeId> {
    match direction {
        TraceDirection::Outbound | TraceDirection::Both if edge.source == *current => {
            Some(&edge.target)
        }
        TraceDirection::Inbound | TraceDirection::Both if edge.target == *current => {
            Some(&edge.source)
        }
        _ => None,
    }
}

fn sort_candidates(graph: &ProjectGraph, candidates: &mut [(NodeId, &GraphEdge)]) {
    candidates.sort_by(|(left_id, left_edge), (right_id, right_edge)| {
        node_qualified_name(graph, left_id)
            .cmp(node_qualified_name(graph, right_id))
            .then_with(|| left_edge.source.cmp(&right_edge.source))
            .then_with(|| left_edge.target.cmp(&right_edge.target))
            .then_with(|| left_edge.kind.cmp(&right_edge.kind))
    });
}

fn trace_hop(
    graph: &ProjectGraph,
    related_id: &NodeId,
    edge: &GraphEdge,
    hop: usize,
) -> Option<TraceHop> {
    let source = graph.node(&edge.source)?;
    let target = graph.node(&edge.target)?;
    let related = graph.node(related_id)?;
    Some(TraceHop {
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
    })
}

fn node_qualified_name<'a>(graph: &'a ProjectGraph, node: &NodeId) -> &'a str {
    graph
        .node(node)
        .map_or("", |node| node.qualified_name.as_str())
}
