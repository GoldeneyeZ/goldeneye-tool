mod common;

use common::Fixture;
use goldeneye_query::{QueryGraphRequest, QueryValue};

#[test]
fn upstream_union_deduplicates_and_union_all_preserves_duplicates() {
    let fixture = Fixture::seeded();
    let union = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) RETURN f.name AS name UNION MATCH (f:Function {name: 'Alpha'}) RETURN f.name AS name",
        ))
        .expect("UNION");
    assert_eq!(
        union.rows,
        vec![vec![QueryValue::String("Alpha".to_owned())]]
    );

    let union_all = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) RETURN f.name AS name UNION ALL MATCH (f:Function {name: 'Alpha'}) RETURN f.name AS name",
        ))
        .expect("UNION ALL");
    assert_eq!(union_all.rows.len(), 2);
    assert_eq!(union_all.rows[0], union_all.rows[1]);
}

#[test]
fn upstream_unwind_expands_lists_before_matching_and_projection() {
    let fixture = Fixture::seeded();
    let result = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "UNWIND [3, 1, 2] AS x MATCH (f:Function {name: 'Alpha'}) RETURN x AS value ORDER BY value",
        ))
        .expect("UNWIND literal list");
    assert_eq!(
        result.rows,
        vec![
            vec![QueryValue::Integer(1)],
            vec![QueryValue::Integer(2)],
            vec![QueryValue::Integer(3)],
        ]
    );
}

#[test]
fn upstream_unwind_accepts_computed_lists_and_empty_lists() {
    let fixture = Fixture::seeded();
    let computed = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "UNWIND split('alpha,beta', ',') AS name MATCH (f:Function) WHERE toLower(f.name) = name RETURN f.name ORDER BY f.name",
        ))
        .expect("UNWIND computed list");
    assert_eq!(computed.rows.len(), 2);

    let empty = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "UNWIND [] AS value MATCH (f:Function) RETURN value",
        ))
        .expect("UNWIND empty list");
    assert!(empty.rows.is_empty());
}
