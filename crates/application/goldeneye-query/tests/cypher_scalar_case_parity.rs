mod common;

use common::Fixture;
use goldeneye_query::{QueryGraphRequest, QueryValue};
use serde_json::json;

#[test]
fn upstream_scalar_string_and_conversion_functions_are_supported() {
    let fixture = Fixture::seeded();
    let result = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) RETURN toLower(f.name) AS lower, toUpper(f.name) AS upper, toString(f.start_line) AS line, size(f.name) AS size, length(f.name) AS length, reverse(f.name) AS reversed",
        ))
        .expect("scalar functions");

    assert_eq!(
        result.rows,
        vec![vec![
            QueryValue::String("alpha".to_owned()),
            QueryValue::String("ALPHA".to_owned()),
            QueryValue::String("1".to_owned()),
            QueryValue::Integer(5),
            QueryValue::Integer(5),
            QueryValue::String("ahplA".to_owned()),
        ]]
    );
}

#[test]
fn upstream_multi_argument_and_null_functions_are_supported() {
    let fixture = Fixture::seeded();
    let result = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) WHERE coalesce(f.missing, 'fallback') = 'fallback' RETURN substring(f.name, 1, 3) AS middle, left(f.name, 2) AS left, right(f.name, 3) AS right, replace(f.name, 'ph', 'F') AS replaced, split('a,b,c', ',') AS parts, trim('  x  ') AS trimmed",
        ))
        .expect("multi-argument functions");

    assert_eq!(
        result.rows,
        vec![vec![
            QueryValue::String("lph".to_owned()),
            QueryValue::String("Al".to_owned()),
            QueryValue::String("pha".to_owned()),
            QueryValue::String("AlFa".to_owned()),
            QueryValue::Json(json!(["a", "b", "c"])),
            QueryValue::String("x".to_owned()),
        ]]
    );
}

#[test]
fn upstream_entity_introspection_functions_are_supported() {
    let fixture = Fixture::seeded();
    let result = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) RETURN labels(f) AS labels, properties(f) AS props, keys(f) AS keys, id(f) AS id",
        ))
        .expect("entity introspection functions");

    assert_eq!(result.rows[0][0], QueryValue::Json(json!(["Function"])));
    assert_eq!(
        result.rows[0][1],
        QueryValue::Json(json!({"language": "rust"}))
    );
    assert_eq!(
        result.rows[0][2],
        QueryValue::Json(json!([
            "name",
            "qualified_name",
            "label",
            "file_path",
            "start_line",
            "end_line",
            "language"
        ]))
    );
    assert_eq!(result.rows[0][3], QueryValue::String("alpha".to_owned()));
}

#[test]
fn upstream_searched_and_simple_case_expressions_are_supported() {
    let fixture = Fixture::seeded();
    let result = fixture
        .engine()
        .query_graph(&QueryGraphRequest::new(
            fixture.project.clone(),
            "MATCH (f:Function {name: 'Alpha'}) RETURN CASE WHEN f.start_line > '2' THEN 'late' WHEN f.start_line = '1' THEN 'first' ELSE 'other' END AS position, CASE f.label WHEN 'Function' THEN 'callable' ELSE 'other' END AS kind",
        ))
        .expect("CASE expressions");

    assert_eq!(
        result.rows,
        vec![vec![
            QueryValue::String("first".to_owned()),
            QueryValue::String("callable".to_owned()),
        ]]
    );
}
