mod common;

use common::Fixture;
use goldeneye_query::{QueryGraphRequest, QueryValue};

#[test]
fn upstream_with_rename_and_grouped_pipeline_are_supported() {
    let fixture = Fixture::seeded();
    let renamed = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) WITH f.name AS fname RETURN fname",
        ))
        .expect("WITH rename");
    assert_eq!(
        renamed.rows,
        vec![vec![QueryValue::String("Alpha".to_owned())]]
    );

    let grouped = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function)-[:CALLS]->(g:Function) WITH f.name AS caller, COUNT(g) AS cnt WHERE cnt >= '1' ORDER BY caller LIMIT 2 RETURN caller, cnt",
        ))
        .expect("WITH aggregate pipeline");
    assert_eq!(
        grouped.rows,
        vec![
            vec![
                QueryValue::String("Alpha".to_owned()),
                QueryValue::Integer(1),
            ],
            vec![
                QueryValue::String("beta".to_owned()),
                QueryValue::Integer(1),
            ],
        ]
    );
}

#[test]
fn upstream_return_star_after_with_uses_the_carried_scope() {
    let fixture = Fixture::seeded();
    let result = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) WITH f.name AS name, f.start_line AS line RETURN *",
        ))
        .expect("RETURN * after WITH");
    assert_eq!(result.columns, vec!["name", "line"]);
    assert_eq!(
        result.rows,
        vec![vec![
            QueryValue::String("Alpha".to_owned()),
            QueryValue::Integer(1),
        ]]
    );
}

#[test]
fn upstream_with_preserves_grouped_nodes_and_distinct_rows() {
    let fixture = Fixture::seeded();
    let grouped = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function)-[:CALLS]->(g:Function) WHERE g.name = 'beta' WITH g, COUNT(*) AS c RETURN g.file_path, g.name, c",
        ))
        .expect("WITH grouped node");
    assert_eq!(grouped.rows.len(), 1);
    assert_eq!(grouped.rows[0][1], QueryValue::String("beta".to_owned()));
    assert_eq!(grouped.rows[0][2], QueryValue::Integer(1));

    let distinct = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) WITH DISTINCT f.label AS label RETURN label",
        ))
        .expect("WITH DISTINCT");
    assert_eq!(
        distinct.rows,
        vec![vec![QueryValue::String("Function".to_owned())]]
    );
}

#[test]
fn upstream_optional_match_preserves_unmatched_bindings_as_null() {
    let fixture = Fixture::seeded();
    let unmatched = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'run'}) OPTIONAL MATCH (f)-[:CALLS]->(g:Function) RETURN f.name, g.name",
        ))
        .expect("unmatched OPTIONAL MATCH");
    assert_eq!(
        unmatched.rows,
        vec![vec![QueryValue::String("run".to_owned()), QueryValue::Null,]]
    );

    let matched = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) OPTIONAL MATCH (f)-[:CALLS]->(g:Function) RETURN f.name, g.name",
        ))
        .expect("matched OPTIONAL MATCH");
    assert_eq!(
        matched.rows,
        vec![vec![
            QueryValue::String("Alpha".to_owned()),
            QueryValue::String("beta".to_owned()),
        ]]
    );
}

#[test]
fn upstream_repeated_match_clauses_join_shared_aliases_and_cross_products() {
    let fixture = Fixture::seeded();
    let joined = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function) MATCH (f)-[:CALLS]->(g:Function) RETURN f.name, g.name",
        ))
        .expect("shared alias join");
    assert_eq!(joined.rows.len(), 3);

    let product = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (m:Module) MATCH (f:Function) WHERE f.name IN ['Alpha', 'beta'] RETURN m.name, f.name",
        ))
        .expect("independent MATCH cross product");
    assert_eq!(product.rows.len(), 2);
}

#[test]
fn upstream_chained_and_comma_separated_patterns_join_atomically() {
    let fixture = Fixture::seeded();
    let chained = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (m:Module)-[:DEFINES]->(f:Function)-[:CALLS]->(g:Function) RETURN f.name, g.name",
        ))
        .expect("chained relationship pattern");
    assert_eq!(chained.rows.len(), 3);

    let comma = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (m:Module), (f:Function {name: 'Alpha'}) RETURN m.name, f.name",
        ))
        .expect("comma-separated patterns");
    assert_eq!(comma.rows.len(), 1);

    let optional_chain = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'run'}) OPTIONAL MATCH (f)-[:CALLS]->(g:Function)-[:CALLS]->(h:Function) RETURN f.name, g.name, h.name",
        ))
        .expect("atomic optional chain");
    assert_eq!(
        optional_chain.rows,
        vec![vec![
            QueryValue::String("run".to_owned()),
            QueryValue::Null,
            QueryValue::Null,
        ]]
    );
}
