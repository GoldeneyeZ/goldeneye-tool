mod common;

use common::Fixture;
use goldeneye_query::{QueryGraphRequest, QueryValue};
use serde_json::json;

#[test]
fn upstream_predicates_and_inline_properties_are_supported() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let result = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) \
             WHERE n.name =~ '^(Alpha|beta)$' AND n.label IN ['Function'] \
             RETURN n.name ORDER BY n.name",
        ))
        .expect("regex and IN query");
    assert_eq!(result.rows, vec![vec![text("Alpha")], vec![text("beta")]]);

    let not_in = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) WHERE n.label NOT IN ['Function', 'Method'] \
             RETURN n.name ORDER BY n.name",
        ))
        .expect("NOT IN query");
    assert_eq!(not_in.rows, vec![vec![text("Café")], vec![text("lib")]]);

    let xor = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) WHERE n.name = 'Alpha' XOR n.name = 'beta' \
             RETURN n.name ORDER BY n.name",
        ))
        .expect("XOR query");
    assert_eq!(xor.rows, vec![vec![text("Alpha")], vec![text("beta")]]);

    let inline = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n:Function {name: 'Alpha'}) RETURN n.qualified_name",
        ))
        .expect("inline property query");
    assert_eq!(inline.rows, vec![vec![text("demo.src.lib.Alpha")]]);
}

#[test]
fn upstream_relationship_alternation_and_bounded_variable_paths_are_supported() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let alternation = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (a)-[r:CALLS|DEFINES]->(b) \
             WHERE a.name = 'main' OR a.name = 'lib' \
             RETURN DISTINCT type(r), b.name ORDER BY b.name",
        ))
        .expect("relationship type alternation");
    assert_eq!(alternation.total, 6);
    assert!(
        alternation
            .rows
            .contains(&vec![text("CALLS"), text("Alpha")])
    );
    assert!(
        alternation
            .rows
            .contains(&vec![text("DEFINES"), text("beta")])
    );

    let variable = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (a:Function)-[:CALLS*1..2]->(b:Function) \
             WHERE a.name = 'main' \
             RETURN DISTINCT b.name ORDER BY b.name",
        ))
        .expect("bounded variable path");
    assert_eq!(variable.rows, vec![vec![text("Alpha")], vec![text("beta")]]);
}

#[test]
fn upstream_return_star_expands_stable_node_columns() {
    let fixture = Fixture::seeded();
    let result = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) RETURN *",
        ))
        .expect("RETURN star query");

    assert_eq!(
        result.columns,
        vec!["f.name", "f.qualified_name", "f.label", "f.file_path"]
    );
    assert_eq!(
        result.rows,
        vec![vec![
            text("Alpha"),
            text("demo.src.lib.Alpha"),
            text("Function"),
            text("src/lib.rs"),
        ]]
    );
}

#[test]
fn upstream_generalized_aggregates_group_and_count_distinct() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let numeric = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) \
             RETURN SUM(f.start_line) AS total, AVG(f.start_line) AS average, \
                    MIN(f.start_line) AS minimum, MAX(f.start_line) AS maximum",
        ))
        .expect("numeric aggregates");
    assert_eq!(
        numeric.rows,
        vec![vec![
            QueryValue::Integer(14),
            QueryValue::Float(3.5),
            QueryValue::Integer(1),
            QueryValue::Integer(6),
        ]]
    );

    let distinct = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) RETURN count(DISTINCT f.label) AS labels",
        ))
        .expect("count distinct");
    assert_eq!(distinct.rows, vec![vec![QueryValue::Integer(1)]]);

    let grouped = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (a)-[r]->(b) RETURN type(r) AS kind, COLLECT(b.name) AS targets \
             ORDER BY kind",
        ))
        .expect("grouped collect");
    assert_eq!(grouped.rows.len(), 2);
    assert_eq!(grouped.rows[0][0], text("CALLS"));
    assert_eq!(grouped.rows[1][0], text("DEFINES"));
    assert!(matches!(grouped.rows[0][1], QueryValue::Json(ref value) if value.is_array()));
    assert_ne!(grouped.rows[0][1], QueryValue::Json(json!([])));
}

fn text(value: &str) -> QueryValue {
    QueryValue::String(value.to_owned())
}
