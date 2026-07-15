use std::collections::BTreeSet;

use goldeneye_domain::{GraphNode, NodeId};

use super::ProjectGraph;
use crate::types::{TraceDirection, TraceHop, TracePathRequest};

pub(super) fn trace_breadth_first(
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
