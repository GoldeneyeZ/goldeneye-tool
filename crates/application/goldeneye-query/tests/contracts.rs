mod common;

use common::Fixture;
use goldeneye_query::{GraphSchemaRequest, IndexStatusRequest, SearchGraphRequest};

#[test]
fn list_status_and_schema_are_project_scoped_and_deterministic() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    let projects = engine.list_projects().expect("list projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project, "demo");
    assert_eq!(projects[0].generation, 1);

    let status = engine
        .index_status(&IndexStatusRequest::new(fixture.project.clone()))
        .expect("index status");
    assert_eq!((status.files, status.nodes, status.edges), (1, 7, 10));
    assert!(status.query_only);

    let schema = engine
        .graph_schema(&GraphSchemaRequest::new(fixture.project.clone()))
        .expect("graph schema");
    assert_eq!(
        schema
            .node_labels
            .iter()
            .map(|entry| (entry.name.as_str(), entry.count))
            .collect::<Vec<_>>(),
        vec![("Function", 4), ("Method", 1), ("Module", 1), ("Struct", 1)]
    );
    assert_eq!(
        schema
            .edge_types
            .iter()
            .map(|entry| (entry.name.as_str(), entry.count))
            .collect::<Vec<_>>(),
        vec![("CALLS", 4), ("DEFINES", 6)]
    );
}

#[test]
fn search_graph_combines_fts_kind_path_and_stable_cursor_pagination() {
    let fixture = Fixture::seeded();
    let engine = fixture.engine();

    let mut request = SearchGraphRequest::new(fixture.project.clone());
    request.query = Some("ALPHA".to_owned());
    request.label = Some("Function".to_owned());
    request.file_pattern = Some(r"^src/.*\.rs$".to_owned());
    request.page.limit = 1;

    let first = engine.search_graph(&request).expect("first page");
    assert_eq!(first.total, 1);
    assert_eq!(first.results[0].qualified_name, "demo.src.lib.Alpha");
    assert!(!first.has_more);

    request.query = None;
    request.name_pattern = Some(".*".to_owned());
    let page_one = engine.search_graph(&request).expect("page one");
    assert!(page_one.has_more);
    let cursor = page_one.next_cursor.clone().expect("next cursor");
    request.page.cursor = Some(cursor);
    let page_two = engine.search_graph(&request).expect("page two");
    assert_ne!(
        page_one.results[0].qualified_name,
        page_two.results[0].qualified_name
    );
    assert_eq!(
        engine.search_graph(&request).expect("repeat page two"),
        page_two
    );
}
