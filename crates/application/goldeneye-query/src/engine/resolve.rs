use std::collections::BTreeMap;

use goldeneye_domain::{GraphNode, NodeId};

use super::{ProjectGraph, node_summary};
use crate::types::{NodeSummary, QueryError};

#[derive(Clone, Copy)]
pub(crate) enum ResolveMode {
    Any,
    Callable,
}

pub(super) fn resolve_symbol_in_graph(
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
