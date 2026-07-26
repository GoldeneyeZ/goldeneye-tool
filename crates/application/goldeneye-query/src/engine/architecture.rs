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
    let mut summaries = languages
        .into_iter()
        .map(|(name, paths)| CountSummary {
            name,
            count: u64::try_from(paths.len()).unwrap_or(u64::MAX),
        })
        .collect::<Vec<_>>();
    sort_count_summaries(&mut summaries);
    summaries
}

fn modules(graph: &ProjectGraph) -> Vec<ArchitectureModule> {
    let mut modules = graph
        .nodes
        .iter()
        .filter(|node| node.label.as_str() == "Module")
        .map(|node| ArchitectureModule {
            name: node.name.clone(),
            qualified_name: node.qualified_name.as_str().to_owned(),
            file_path: node.file_path.as_ref().map(|path| path.as_str().to_owned()),
            defined_symbols: graph.define_counts.get(&node.id).copied().unwrap_or(0),
        })
        .collect::<Vec<_>>();
    sort_modules(&mut modules);
    modules
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
    let mut types = graph
        .nodes
        .iter()
        .filter(|node| TYPE_LABELS.contains(&node.label.as_str()))
        .map(|node| node_summary(node, None, &graph.degrees, Vec::new()))
        .collect::<Vec<_>>();
    sort_nodes_by_signal(&mut types);
    types
}

fn entry_points(graph: &ProjectGraph) -> Vec<NodeSummary> {
    let mut entry_points = graph
        .nodes
        .iter()
        .filter(|node| is_entry_point(node))
        .map(|node| node_summary(node, None, &graph.degrees, Vec::new()))
        .collect::<Vec<_>>();
    sort_nodes_by_signal(&mut entry_points);
    entry_points
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
    let mut summaries = edge_counts
        .into_iter()
        .map(|(name, count)| CountSummary { name, count })
        .collect::<Vec<_>>();
    sort_count_summaries(&mut summaries);
    summaries
}

fn sort_count_summaries(summaries: &mut [CountSummary]) {
    summaries.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.name.cmp(&right.name))
    });
}

fn sort_modules(modules: &mut [ArchitectureModule]) {
    modules.sort_by(|left, right| {
        right
            .defined_symbols
            .cmp(&left.defined_symbols)
            .then_with(|| left.qualified_name.cmp(&right.qualified_name))
    });
}

fn sort_nodes_by_signal(nodes: &mut [NodeSummary]) {
    nodes.sort_by(|left, right| {
        right
            .in_degree
            .saturating_add(right.out_degree)
            .cmp(&left.in_degree.saturating_add(left.out_degree))
            .then_with(|| right.in_degree.cmp(&left.in_degree))
            .then_with(|| right.out_degree.cmp(&left.out_degree))
            .then_with(|| left.qualified_name.cmp(&right.qualified_name))
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{sort_count_summaries, sort_modules, sort_nodes_by_signal};
    use crate::types::{ArchitectureModule, CountSummary, NodeSummary};

    #[test]
    fn count_summaries_rank_by_count_then_name() {
        let mut summaries = vec![
            CountSummary {
                name: "zeta".to_owned(),
                count: 3,
            },
            CountSummary {
                name: "beta".to_owned(),
                count: 8,
            },
            CountSummary {
                name: "alpha".to_owned(),
                count: 8,
            },
        ];

        sort_count_summaries(&mut summaries);

        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta", "zeta"]
        );
    }

    #[test]
    fn modules_rank_by_defined_symbols_then_qualified_name() {
        let mut modules = vec![module("zeta", 3), module("beta", 8), module("alpha", 8)];

        sort_modules(&mut modules);

        assert_eq!(
            modules
                .iter()
                .map(|module| module.qualified_name.as_str())
                .collect::<Vec<_>>(),
            ["alpha", "beta", "zeta"]
        );
    }

    #[test]
    fn nodes_rank_by_total_then_inbound_then_outbound_degree() {
        let mut nodes = vec![
            node("zeta", 2, 2),
            node("outbound", 1, 7),
            node("beta", 4, 4),
            node("alpha", 4, 4),
            node("inbound", 7, 1),
        ];

        sort_nodes_by_signal(&mut nodes);

        assert_eq!(
            nodes
                .iter()
                .map(|node| node.qualified_name.as_str())
                .collect::<Vec<_>>(),
            ["inbound", "alpha", "beta", "outbound", "zeta"]
        );
    }

    fn module(qualified_name: &str, defined_symbols: usize) -> ArchitectureModule {
        ArchitectureModule {
            name: qualified_name.to_owned(),
            qualified_name: qualified_name.to_owned(),
            file_path: None,
            defined_symbols,
        }
    }

    fn node(qualified_name: &str, in_degree: usize, out_degree: usize) -> NodeSummary {
        NodeSummary {
            id: qualified_name.to_owned(),
            name: qualified_name.to_owned(),
            qualified_name: qualified_name.to_owned(),
            label: "Class".to_owned(),
            file_path: None,
            start_byte: None,
            end_byte: None,
            start_line: None,
            end_line: None,
            generation: 0,
            in_degree,
            out_degree,
            rank: None,
            connected_names: Vec::new(),
            properties: BTreeMap::new(),
        }
    }
}
