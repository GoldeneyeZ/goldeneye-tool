use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};

use super::super::{
    MAX_INTERMEDIATE_BINDINGS,
    ast::{EdgeDirection, EdgeMatch, EdgePattern, MatchPattern, NodePattern},
    unsupported,
};
use super::{binding::Binding, compare::values_equal, reference::node_property};
use crate::types::QueryError;

pub(super) fn build_bindings_bounded<'a>(
    pattern: &MatchPattern,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    let mut bindings = match pattern {
        MatchPattern::Node(pattern) => nodes
            .iter()
            .filter(|node| node_matches(node, pattern, degrees))
            .map(|node| Binding {
                nodes: BTreeMap::from([(pattern.alias.clone(), node)]),
                edges: BTreeMap::new(),
                values: BTreeMap::new(),
                all_nodes: Some(nodes),
                all_edges: Some(edges),
            })
            .collect(),
        MatchPattern::Edge(pattern) => {
            let EdgeMatch { left, edge, right } = pattern.as_ref();
            if edge.min_hops != 1 || edge.max_hops != 1 {
                return build_variable_bindings(pattern, nodes, edges, degrees);
            }
            let nodes_by_id: BTreeMap<&NodeId, &GraphNode> =
                nodes.iter().map(|node| (&node.id, node)).collect();
            let mut bindings = Vec::new();
            for graph_edge in edges.iter().filter(|graph_edge| {
                edge.kinds.is_empty()
                    || edge
                        .kinds
                        .iter()
                        .any(|kind| graph_edge.kind.as_str() == kind)
            }) {
                let Some(source) = nodes_by_id.get(&graph_edge.source).copied() else {
                    continue;
                };
                let Some(target) = nodes_by_id.get(&graph_edge.target).copied() else {
                    continue;
                };
                match edge.direction {
                    EdgeDirection::Outbound => push_edge_binding(
                        &mut bindings,
                        left,
                        right,
                        edge,
                        source,
                        target,
                        graph_edge,
                        degrees,
                    ),
                    EdgeDirection::Inbound => push_edge_binding(
                        &mut bindings,
                        left,
                        right,
                        edge,
                        target,
                        source,
                        graph_edge,
                        degrees,
                    ),
                    EdgeDirection::Undirected => {
                        push_edge_binding(
                            &mut bindings,
                            left,
                            right,
                            edge,
                            source,
                            target,
                            graph_edge,
                            degrees,
                        );
                        if source.id != target.id {
                            push_edge_binding(
                                &mut bindings,
                                left,
                                right,
                                edge,
                                target,
                                source,
                                graph_edge,
                                degrees,
                            );
                        }
                    }
                }
            }
            bindings
        }
    };
    if bindings.len() > MAX_INTERMEDIATE_BINDINGS {
        return Err(unsupported("query exceeds intermediate binding safety cap"));
    }
    for binding in &mut bindings {
        binding.all_nodes = Some(nodes);
        binding.all_edges = Some(edges);
    }
    Ok(bindings)
}

fn push_edge_binding<'a>(
    bindings: &mut Vec<Binding<'a>>,
    left_pattern: &NodePattern,
    right_pattern: &NodePattern,
    edge_pattern: &EdgePattern,
    left: &'a GraphNode,
    right: &'a GraphNode,
    edge: &'a GraphEdge,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) {
    if !node_matches(left, left_pattern, degrees) || !node_matches(right, right_pattern, degrees) {
        return;
    }
    if left_pattern.alias == right_pattern.alias && left.id != right.id {
        return;
    }
    let mut edges = BTreeMap::new();
    if let Some(alias) = &edge_pattern.alias {
        edges.insert(alias.clone(), edge);
    }
    bindings.push(Binding {
        nodes: BTreeMap::from([
            (left_pattern.alias.clone(), left),
            (right_pattern.alias.clone(), right),
        ]),
        edges,
        values: BTreeMap::new(),
        all_nodes: None,
        all_edges: None,
    });
}

fn build_variable_bindings<'a>(
    pattern: &EdgeMatch,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    struct Frame<'a> {
        start: &'a GraphNode,
        current: &'a GraphNode,
        depth: usize,
        used_edges: BTreeSet<usize>,
        last_edge: Option<&'a GraphEdge>,
    }

    let nodes_by_id: BTreeMap<&NodeId, &GraphNode> =
        nodes.iter().map(|node| (&node.id, node)).collect();
    let mut bindings = Vec::new();
    for start in nodes
        .iter()
        .filter(|node| node_matches(node, &pattern.left, degrees))
    {
        let mut stack = vec![Frame {
            start,
            current: start,
            depth: 0,
            used_edges: BTreeSet::new(),
            last_edge: None,
        }];
        while let Some(frame) = stack.pop() {
            if frame.depth >= pattern.edge.min_hops
                && node_matches(frame.current, &pattern.right, degrees)
                && (pattern.left.alias != pattern.right.alias || frame.start.id == frame.current.id)
            {
                let mut bound_edges = BTreeMap::new();
                if let (Some(alias), Some(edge)) = (&pattern.edge.alias, frame.last_edge) {
                    bound_edges.insert(alias.clone(), edge);
                }
                bindings.push(Binding {
                    nodes: BTreeMap::from([
                        (pattern.left.alias.clone(), frame.start),
                        (pattern.right.alias.clone(), frame.current),
                    ]),
                    edges: bound_edges,
                    values: BTreeMap::new(),
                    all_nodes: Some(nodes),
                    all_edges: Some(edges),
                });
                if bindings.len() > MAX_INTERMEDIATE_BINDINGS {
                    return Err(unsupported("query exceeds intermediate binding safety cap"));
                }
            }
            if frame.depth >= pattern.edge.max_hops {
                continue;
            }
            for (edge_index, edge) in edges.iter().enumerate().rev() {
                if frame.used_edges.contains(&edge_index)
                    || (!pattern.edge.kinds.is_empty()
                        && !pattern
                            .edge
                            .kinds
                            .iter()
                            .any(|kind| edge.kind.as_str() == kind))
                {
                    continue;
                }
                let next_ids: Vec<&NodeId> = match pattern.edge.direction {
                    EdgeDirection::Outbound | EdgeDirection::Undirected
                        if edge.source == frame.current.id =>
                    {
                        vec![&edge.target]
                    }
                    EdgeDirection::Inbound | EdgeDirection::Undirected
                        if edge.target == frame.current.id =>
                    {
                        vec![&edge.source]
                    }
                    _ => Vec::new(),
                };
                for next_id in next_ids {
                    let Some(next) = nodes_by_id.get(next_id).copied() else {
                        continue;
                    };
                    let mut used_edges = frame.used_edges.clone();
                    used_edges.insert(edge_index);
                    stack.push(Frame {
                        start: frame.start,
                        current: next,
                        depth: frame.depth + 1,
                        used_edges,
                        last_edge: Some(edge),
                    });
                }
            }
        }
    }
    Ok(bindings)
}

pub(in crate::cypher) fn node_matches(
    node: &GraphNode,
    pattern: &NodePattern,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> bool {
    (pattern.labels.is_empty()
        || pattern
            .labels
            .iter()
            .any(|label| node.label.as_str() == label))
        && pattern.properties.iter().all(|(property, expected)| {
            values_equal(
                &node_property(node, std::slice::from_ref(property), degrees),
                expected,
            )
        })
}
