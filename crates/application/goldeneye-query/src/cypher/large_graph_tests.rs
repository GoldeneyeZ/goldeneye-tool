//! Queries over a graph larger than the intermediate binding cap.

use std::collections::BTreeMap;

use goldeneye_domain::{
    EdgeKind, Generation, GraphEdge, GraphNode, NodeId, NodeLabel, ProjectId, ProjectRelativePath,
    QualifiedName,
};

use super::{MAX_INTERMEDIATE_BINDINGS, execute};
use crate::types::{QueryError, QueryGraphRequest, QueryValue};

fn project() -> ProjectId {
    ProjectId::new("demo").expect("project ID")
}

fn node(index: usize, file: &str) -> GraphNode {
    GraphNode::new(
        project(),
        NodeId::new(format!("n{index}")).expect("node ID"),
        NodeLabel::new("Variable").expect("node label"),
        format!("key{index}"),
        QualifiedName::new(format!("demo.data.key{index}")).expect("qualified name"),
        Some(ProjectRelativePath::new(file).expect("relative path")),
        None,
        Generation::new(0),
    )
    .expect("graph node")
}

/// One more node than the cap: all but the last two are keys of a data file.
fn large_graph() -> (Vec<GraphNode>, Vec<GraphEdge>) {
    let total = MAX_INTERMEDIATE_BINDINGS + 1;
    let nodes = (0..total)
        .map(|index| {
            node(
                index,
                if index + 2 >= total {
                    "src/pairs.py"
                } else {
                    "data/rows.json"
                },
            )
        })
        .collect::<Vec<_>>();
    // Every node is defined by the first one, except the last, which is
    // defined by the one before it.
    let edges = (1..total)
        .map(|index| {
            let owner = if index + 1 == total { index - 1 } else { 0 };
            GraphEdge::new(
                project(),
                NodeId::new(format!("n{owner}")).expect("source ID"),
                NodeId::new(format!("n{index}")).expect("target ID"),
                EdgeKind::new("DEFINES").expect("edge kind"),
                Generation::new(0),
            )
        })
        .collect();
    (nodes, edges)
}

fn run(query: &str, nodes: &[GraphNode], edges: &[GraphEdge]) -> Result<Vec<String>, QueryError> {
    let request = QueryGraphRequest::new(project(), query);
    execute(&request, nodes, edges, &BTreeMap::new()).map(|result| {
        result
            .rows
            .into_iter()
            .map(|row| match row.into_iter().next() {
                Some(QueryValue::String(text)) => text,
                other => format!("{other:?}"),
            })
            .collect()
    })
}

#[test]
fn a_filter_on_one_pattern_is_applied_before_the_binding_cap() {
    let (nodes, edges) = large_graph();
    let last = MAX_INTERMEDIATE_BINDINGS;

    // The lookup GCAL makes for every file and line: no label, a few matches.
    let in_file = run(
        "MATCH (n) WHERE n.file_path = 'src/pairs.py' RETURN n.name ORDER BY n.name",
        &nodes,
        &edges,
    )
    .expect("nodes of one file");
    assert_eq!(
        in_file,
        // Ordered as text.
        vec![format!("key{last}"), format!("key{}", last - 1)]
    );

    // The member lookup: one owner out of more edges than the cap.
    let members = run(
        &format!(
            "MATCH (c)-[:DEFINES]->(m) WHERE c.qualified_name = 'demo.data.key{}' RETURN m.name",
            last - 1
        ),
        &nodes,
        &edges,
    )
    .expect("members of one owner");
    assert_eq!(members, vec![format!("key{last}")]);

    // With LIMIT the query takes the single-node fast path; the cap counts the same way there.
    let limited = run(
        "MATCH (n) WHERE n.file_path = 'src/pairs.py' RETURN n.name LIMIT 8",
        &nodes,
        &edges,
    )
    .expect("nodes of one file, limited");
    assert_eq!(limited.len(), 2);
}

#[test]
fn a_query_that_keeps_more_than_the_cap_still_fails() {
    let (nodes, edges) = large_graph();

    for query in [
        "MATCH (n) RETURN n.name",
        "MATCH (n) WHERE n.name STARTS WITH 'key' RETURN n.name",
        "MATCH (n) RETURN n.name LIMIT 8",
        "MATCH (n) WHERE n.name STARTS WITH 'key' RETURN n.name LIMIT 8",
    ] {
        let error = run(query, &nodes, &edges).expect_err("over the cap");
        assert!(
            error
                .to_string()
                .contains("query exceeds intermediate binding safety cap"),
            "{query}: {error}"
        );
    }
}
