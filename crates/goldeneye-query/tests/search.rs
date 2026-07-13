mod common;

use common::Fixture;
use goldeneye_query::{QueryError, SearchGraphRequest};

#[test]
fn unicode_fts_is_case_insensitive_while_regex_case_is_explicit() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    let mut request = SearchGraphRequest::new(fixture.project.clone());
    request.query = Some("CAFÉ".to_owned());
    let fts = engine.search_graph(&request).expect("Unicode FTS");
    assert_eq!(fts.results[0].qualified_name, "demo.src.lib.Café");

    let mut request = SearchGraphRequest::new(fixture.project.clone());
    request.name_pattern = Some("^café$".to_owned());
    assert!(
        engine
            .search_graph(&request)
            .expect("case-sensitive regex")
            .results
            .is_empty()
    );
    request.name_pattern = Some("(?i)^café$".to_owned());
    assert_eq!(
        engine
            .search_graph(&request)
            .expect("case-insensitive regex")
            .results[0]
            .name,
        "Café"
    );
}

#[test]
fn duplicate_short_names_remain_distinct_and_sorted_by_qualified_name() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request = SearchGraphRequest::new(fixture.project.clone());
    request.name_pattern = Some("^run$".to_owned());

    let page = engine.search_graph(&request).expect("duplicate names");
    assert_eq!(page.total, 2);
    assert_eq!(
        page.results
            .iter()
            .map(|node| node.qualified_name.as_str())
            .collect::<Vec<_>>(),
        vec!["demo.src.lib.Café.run", "demo.src.lib.run"]
    );
}

#[test]
fn relationship_degree_entrypoint_and_connected_filters_compose() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request = SearchGraphRequest::new(fixture.project.clone());
    request.name_pattern = Some(".*".to_owned());
    request.label = Some("Function".to_owned());
    request.relationship = Some("CALLS".to_owned());
    request.min_degree = Some(2);
    request.exclude_entry_points = true;

    let filtered = engine.search_graph(&request).expect("combined filters");
    assert_eq!(
        filtered
            .results
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "beta"]
    );

    let mut connected = SearchGraphRequest::new(fixture.project.clone());
    connected.name_pattern = Some("^main$".to_owned());
    connected.include_connected = true;
    let connected = engine.search_graph(&connected).expect("connected nodes");
    assert_eq!(
        connected
            .results
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        vec!["main"]
    );
    assert_eq!(connected.results[0].connected_names, vec!["Alpha"]);
}

#[test]
fn cursor_is_bound_to_filters_and_rejects_tampering() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();
    let mut request = SearchGraphRequest::new(fixture.project.clone());
    request.name_pattern = Some(".*".to_owned());
    request.page.limit = 1;
    let first = engine.search_graph(&request).expect("first page");

    request.page.cursor = first.next_cursor;
    request.label = Some("Function".to_owned());
    assert!(matches!(
        engine.search_graph(&request),
        Err(QueryError::CursorMismatch)
    ));

    let mut presence = SearchGraphRequest::new(fixture.project.clone());
    presence.name_pattern = Some(".*".to_owned());
    presence.page.limit = 1;
    let first = engine.search_graph(&presence).expect("presence first page");
    presence.page.cursor = first.next_cursor;
    presence.max_degree = Some(0);
    assert!(matches!(
        engine.search_graph(&presence),
        Err(QueryError::CursorMismatch)
    ));
}
