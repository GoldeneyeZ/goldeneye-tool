use std::collections::{BTreeMap, BTreeSet};

use crate::types::{ArchitectureModule, CountSummary, NodeSummary};

use super::{ProjectGraph, node_summary};

pub(super) struct ArchitectureSummary {
    pub(super) total_nodes: usize,
    pub(super) total_edges: usize,
    pub(super) languages: Vec<CountSummary>,
    pub(super) modules: Vec<ArchitectureModule>,
    pub(super) types: Vec<NodeSummary>,
    pub(super) entry_points: Vec<NodeSummary>,
    pub(super) edge_types: Vec<CountSummary>,
}

impl ArchitectureSummary {
    pub(super) fn from_graph(graph: &ProjectGraph) -> Self {
        Self {
            total_nodes: graph.nodes.len(),
            total_edges: graph.edges.len(),
            languages: languages(graph),
            modules: modules(graph),
            types: types(graph),
            entry_points: entry_points(graph),
            edge_types: edge_types(graph),
        }
    }
}

fn languages(graph: &ProjectGraph) -> Vec<CountSummary> {
    let mut languages: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in &graph.nodes {
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
    languages
        .into_iter()
        .map(|(name, paths)| CountSummary {
            name,
            count: u64::try_from(paths.len()).unwrap_or(u64::MAX),
        })
        .collect()
}

fn modules(graph: &ProjectGraph) -> Vec<ArchitectureModule> {
    graph
        .nodes
        .iter()
        .filter(|node| node.label.as_str() == "Module")
        .map(|node| ArchitectureModule {
            name: node.name.clone(),
            qualified_name: node.qualified_name.as_str().to_owned(),
            file_path: node.file_path.as_ref().map(|path| path.as_str().to_owned()),
            defined_symbols: graph.define_counts.get(&node.id).copied().unwrap_or(0),
        })
        .collect()
}

fn types(graph: &ProjectGraph) -> Vec<NodeSummary> {
    const TYPE_LABELS: [&str; 7] = [
        "Class",
        "Enum",
        "Interface",
        "Struct",
        "Trait",
        "Type",
        "TypeAlias",
    ];
    graph
        .nodes
        .iter()
        .filter(|node| TYPE_LABELS.contains(&node.label.as_str()))
        .map(|node| node_summary(node, None, &graph.degrees, Vec::new()))
        .collect()
}

fn entry_points(graph: &ProjectGraph) -> Vec<NodeSummary> {
    graph
        .nodes
        .iter()
        .filter(|node| is_entry_point(node))
        .map(|node| node_summary(node, None, &graph.degrees, Vec::new()))
        .take(20)
        .collect()
}

fn is_entry_point(node: &goldeneye_domain::GraphNode) -> bool {
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
}

fn edge_types(graph: &ProjectGraph) -> Vec<CountSummary> {
    let mut edge_counts: BTreeMap<String, u64> = BTreeMap::new();
    for edge in &graph.edges {
        *edge_counts
            .entry(edge.kind.as_str().to_owned())
            .or_default() += 1;
    }
    edge_counts
        .into_iter()
        .map(|(name, count)| CountSummary { name, count })
        .collect()
}
