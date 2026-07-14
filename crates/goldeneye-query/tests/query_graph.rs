mod common;

use common::Fixture;
use goldeneye_query::{QueryError, QueryGraphRequest, QueryValue};

#[test]
fn node_match_filters_projects_orders_and_projects_typed_values() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let result = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) \
             WHERE f.name CONTAINS 'a' AND f.file_path STARTS WITH 'src/' \
             RETURN f.qualified_name, f.name \
             ORDER BY f.qualified_name ASC LIMIT 10",
        ))
        .expect("node query");

    assert_eq!(result.columns, vec!["f.qualified_name", "f.name"]);
    assert_eq!(result.total, 3);
    assert_eq!(
        result.rows,
        vec![
            vec![text("demo.src.lib.Alpha"), text("Alpha")],
            vec![text("demo.src.lib.beta"), text("beta")],
            vec![text("demo.src.lib.main"), text("main")],
        ]
    );
    assert!(!result.truncated);
}

#[test]
fn one_hop_outbound_and_inbound_patterns_bind_nodes_and_edge_type() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let outbound = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (caller:Function)-[r:CALLS]->(callee:Function) \
             WHERE caller.name = 'main' \
             RETURN caller.qualified_name, type(r), callee.qualified_name",
        ))
        .expect("outbound edge query");
    let inbound = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (callee:Function)<-[r:CALLS]-(caller:Function) \
             WHERE caller.name = 'main' \
             RETURN caller.qualified_name, type(r), callee.qualified_name",
        ))
        .expect("inbound edge query");

    let expected = vec![vec![
        text("demo.src.lib.main"),
        text("CALLS"),
        text("demo.src.lib.Alpha"),
    ]];
    assert_eq!(outbound.rows, expected);
    assert_eq!(inbound.rows, expected);
}

#[test]
fn distinct_properties_boole_and_count_aggregate_are_supported() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let distinct = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) RETURN DISTINCT n.label ORDER BY n.label",
        ))
        .expect("distinct query");
    assert_eq!(
        distinct.rows,
        vec![
            vec![text("Function")],
            vec![text("Method")],
            vec![text("Module")],
            vec![text("Struct")],
        ]
    );

    let entry = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) WHERE f.is_entry_point = true \
             RETURN f.name, f.is_entry_point",
        ))
        .expect("boolean property query");
    assert_eq!(entry.rows, vec![vec![text("main"), QueryValue::Bool(true)]]);

    let count = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) RETURN count(f)",
        ))
        .expect("count query");
    assert_eq!(count.rows, vec![vec![QueryValue::Integer(4)]]);
}

#[test]
fn max_rows_skip_and_limit_bound_materialization() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request = QueryGraphRequest::new(
        fixture.project.clone(),
        "MATCH (n) RETURN n.qualified_name ORDER BY n.qualified_name SKIP 1",
    );
    request.max_rows = 2;
    let bounded = engine.query_graph(&request).expect("bounded query");
    assert_eq!(bounded.total, 6);
    assert_eq!(bounded.rows.len(), 2);
    assert!(bounded.truncated);

    let limited = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) RETURN n.name ORDER BY n.qualified_name LIMIT 1",
        ))
        .expect("query limit");
    assert_eq!(limited.rows.len(), 1);
    assert!(limited.truncated);
}

#[test]
fn simple_limit_without_order_preserves_rows_total_and_truncation() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let result = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) RETURN n.name LIMIT 1",
        ))
        .expect("bounded query without explicit order");

    assert_eq!(result.rows, vec![vec![text("Alpha")]]);
    assert_eq!(result.total, 7);
    assert!(result.truncated);
}

#[test]
fn direct_node_limit_preserves_sorted_rows_skip_and_total() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let result = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) RETURN n SKIP 1 LIMIT 2",
        ))
        .expect("bounded direct-node query");

    let ids = result
        .rows
        .iter()
        .map(|row| match row.as_slice() {
            [QueryValue::Node(node)] => node.id.as_str(),
            _ => panic!("expected one node value"),
        })
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 2);
    assert!(ids.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(result.total, 6);
    assert!(result.truncated);
}

#[test]
fn simple_limit_fast_path_preserves_filter_skip_and_total() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let result = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) WHERE f.name CONTAINS 'a' RETURN f.name SKIP 1 LIMIT 1",
        ))
        .expect("filtered bounded query without explicit order");

    assert_eq!(result.rows, vec![vec![text("beta")]]);
    assert_eq!(result.total, 2);
    assert!(result.truncated);
}

#[test]
fn simple_limit_fast_path_preserves_projection_error() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let error = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) RETURN unsupported(n) LIMIT 1",
        ))
        .expect_err("unsupported projection function");

    assert!(matches!(
        error,
        QueryError::UnsupportedQuery { ref message }
            if message == "unsupported function unsupported"
    ));
}

#[test]
fn mutating_syntax_fails_closed_without_false_literal_hits() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    for query in [
        "CREATE (n)",
        "MATCH (n) DELETE n RETURN n",
        "MATCH (n) SET n.name = 'changed' RETURN n",
        "MATCH (n) MERGE (m) RETURN n",
    ] {
        assert!(matches!(
            engine.query_graph(&QueryGraphRequest::new(fixture.project.clone(), query)),
            Err(QueryError::MutatingQuery { .. })
        ));
    }
    assert_eq!(
        engine
            .index_status(&goldeneye_query::IndexStatusRequest::new(
                fixture.project.clone()
            ))
            .expect("unchanged status")
            .nodes,
        7
    );

    let literal = engine
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (n) WHERE n.name = 'CREATE' RETURN n.name",
        ))
        .expect("mutation word inside literal");
    assert!(literal.rows.is_empty());
}

fn text(value: &str) -> QueryValue {
    QueryValue::String(value.to_owned())
}
