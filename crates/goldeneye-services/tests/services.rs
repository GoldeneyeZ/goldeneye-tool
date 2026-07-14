use std::fs;
use std::sync::{Arc, Mutex};

use goldeneye_services::{
    ArchitectureRequest, CancellationToken, CodeSnippetRequest, CreateFileRequest,
    GraphSchemaRequest, IndexRepositoryRequest, IndexRepositoryResult, IndexStatusRequest,
    InspectSyntaxRequest, NodeContentRequest, OperationHooks, PageRequest, ProjectId,
    ProjectRelativePath, QueryGraphRequest, SearchCodeRequest, SearchCodeResult,
    SearchGraphRequest, ServiceConfig, ServiceErrorCode, Services, TraceDirection,
    TracePathRequest,
};
use tempfile::TempDir;

fn write_fixture(root: &std::path::Path) {
    fs::create_dir_all(root.join("src")).expect("create source directory");
    fs::write(
        root.join("src/lib.rs"),
        "pub fn helper() -> usize { 1 }\npub fn entry() -> usize { helper() }\n",
    )
    .expect("write fixture");
}

fn verify_read_surfaces(services: &Services, indexed: &IndexRepositoryResult) -> ProjectId {
    let project = ProjectId::new(indexed.project.clone()).expect("project ID");
    assert_eq!(services.list_projects().expect("projects").len(), 1);
    let status = services
        .index_status(&IndexStatusRequest::new(project.clone()))
        .expect("status");
    assert_eq!(status.nodes, indexed.nodes);
    assert!(
        !services
            .get_graph_schema(&GraphSchemaRequest::new(project.clone()))
            .expect("schema")
            .node_labels
            .is_empty()
    );

    let mut first_page = SearchGraphRequest::new(project.clone());
    first_page.page = PageRequest {
        limit: 1,
        offset: 0,
        cursor: None,
    };
    let first = services.search_graph(&first_page).expect("first page");
    assert_eq!(first.results.len(), 1);
    assert!(first.has_more);
    let mut second_page = first_page.clone();
    second_page.page.cursor.clone_from(&first.next_cursor);
    let second = services.search_graph(&second_page).expect("second page");
    assert_ne!(first.results[0].id, second.results[0].id);

    let mut helper_search = SearchGraphRequest::new(project.clone());
    helper_search.name_pattern = Some("^helper$".to_owned());
    let helper = services
        .search_graph(&helper_search)
        .expect("helper search")
        .results
        .into_iter()
        .find(|node| node.label == "Function")
        .expect("helper function");
    let snippet = services
        .get_code_snippet(&CodeSnippetRequest::new(
            project.clone(),
            helper.qualified_name.clone(),
        ))
        .expect("snippet");
    assert!(snippet.source.contains("pub fn helper"));

    let trace = services
        .trace_path(&TracePathRequest::new(
            project.clone(),
            helper.qualified_name,
            TraceDirection::Inbound,
        ))
        .expect("trace");
    assert!(!trace.paths.is_empty());
    let alias = services
        .trace_call_path(&TracePathRequest::new(
            project.clone(),
            trace.origin.qualified_name.clone(),
            TraceDirection::Inbound,
        ))
        .expect("trace alias");
    assert_eq!(trace, alias);

    let architecture = services
        .get_architecture(&ArchitectureRequest::new(project.clone()))
        .expect("architecture");
    assert!(architecture.total_nodes > 0);
    let query = services
        .query_graph(&QueryGraphRequest::new(
            project.clone(),
            "MATCH (f:Function) RETURN f.name ORDER BY f.name",
        ))
        .expect("query graph");
    assert!(!query.rows.is_empty());
    let SearchCodeResult::Matches(code) = services
        .search_code(&SearchCodeRequest::new(project.clone(), "helper"))
        .expect("search code")
    else {
        panic!("expected search matches");
    };
    assert!(code.total_grep_matches >= 2);
    assert!(code.results.iter().any(|result| result.node == "helper"));
    project
}

#[test]
fn semantic_model_configuration_has_upstream_defaults_and_explicit_overrides() {
    let config = ServiceConfig::new("graph.db", ".");
    assert!(!config.semantic_enabled());
    assert_eq!(config.semantic_threshold(), 0.75);

    let configured = config.with_semantic_config(true, 0.82);
    assert!(configured.semantic_enabled());
    assert_eq!(configured.semantic_threshold(), 0.82);
}

#[test]
fn index_then_every_read_surface_paginates_and_reopens() {
    let temp = TempDir::new().expect("temp directory");
    let allowed = temp.path().join("allowed");
    let repo = allowed.join("fixture");
    write_fixture(&repo);
    let database = temp.path().join("state/graph.db");
    let config = ServiceConfig::new(&database, &allowed).with_allowed_root(&allowed);
    let services = Services::new(config.clone());

    let events = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&events);
    let hooks = OperationHooks::default().with_progress(move |event| {
        captured.lock().expect("progress lock").push(event.stage);
    });
    let indexed = services
        .index_repository_with_hooks(&IndexRepositoryRequest::new("fixture"), &hooks)
        .expect("index fixture");
    assert_eq!(indexed.status.as_str(), "indexed");
    assert!(indexed.nodes > 0);
    assert_eq!(
        events.lock().expect("events").first().map(String::as_str),
        Some("resolving")
    );
    assert_eq!(
        events.lock().expect("events").last().map(String::as_str),
        Some("complete")
    );

    let project = verify_read_surfaces(&services, &indexed);

    drop(services);
    let reopened = Services::new(config);
    assert_eq!(
        reopened.list_projects().expect("reopened projects").len(),
        1
    );
    assert_eq!(
        reopened
            .index_status(&IndexStatusRequest::new(project))
            .expect("reopened status")
            .nodes,
        indexed.nodes
    );
}

#[test]
fn allowed_root_unknown_project_and_cancellation_are_typed() {
    let temp = TempDir::new().expect("temp directory");
    let allowed = temp.path().join("allowed");
    let outside = temp.path().join("outside");
    write_fixture(&allowed.join("inside"));
    write_fixture(&outside);
    let services = Services::new(
        ServiceConfig::new(temp.path().join("graph.db"), &allowed).with_allowed_root(&allowed),
    );

    let outside_error = services
        .index_repository(&IndexRepositoryRequest::new(&outside))
        .expect_err("outside root must fail");
    assert_eq!(outside_error.code(), ServiceErrorCode::Forbidden);

    let missing = ProjectId::new("missing").expect("missing project ID");
    let missing_error = services
        .index_status(&IndexStatusRequest::new(missing))
        .expect_err("unknown project must fail");
    assert_eq!(missing_error.code(), ServiceErrorCode::NotFound);

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let cancelled = services
        .index_repository_with_hooks(
            &IndexRepositoryRequest::new("inside"),
            &OperationHooks::new(cancellation),
        )
        .expect_err("cancelled request");
    assert_eq!(cancelled.code(), ServiceErrorCode::Cancelled);
}

#[test]
fn structural_edits_refresh_source_graph_and_reject_stale_or_existing_targets() {
    let temp = TempDir::new().expect("temp directory");
    let allowed = temp.path().join("allowed");
    let repo = allowed.join("fixture");
    write_fixture(&repo);
    let services = Services::new(
        ServiceConfig::new(temp.path().join("graph.db"), &allowed).with_allowed_root(&allowed),
    );
    let indexed = services
        .index_repository(&IndexRepositoryRequest::new(&repo))
        .expect("index fixture");
    let project = ProjectId::new(indexed.project).expect("project ID");
    let path = ProjectRelativePath::new("src/lib.rs").expect("relative path");
    let mut inspect_request = InspectSyntaxRequest::new(project.clone(), path.clone());
    inspect_request.inspect.preview_chars = 96;
    let inspection = services
        .inspect_syntax(&inspect_request)
        .expect("inspect source");
    let helper_index = inspection
        .syntax
        .nodes
        .iter()
        .position(|node| {
            node.kind == "function_item"
                && node
                    .preview
                    .as_deref()
                    .is_some_and(|preview| preview.contains("fn helper"))
        })
        .expect("helper syntax node");
    let helper = inspection.locators[helper_index].clone();

    let replacement = NodeContentRequest::new(
        "replace-helper",
        helper.clone(),
        "pub fn helper() -> usize { 2 }",
    );
    let mutation = services.replace_node(&replacement).expect("replace helper");
    assert_ne!(mutation.old_file_hash, Some(mutation.new_file_hash));
    assert!(!mutation.changed_syntax_ids.is_empty());
    assert!(!mutation.changed_graph_ids.is_empty());
    assert_eq!(
        fs::read_to_string(repo.join("src/lib.rs")).expect("read replacement"),
        "pub fn helper() -> usize { 2 }\npub fn entry() -> usize { helper() }\n"
    );

    let stale = services
        .replace_node(&replacement)
        .expect_err("stale locator must fail");
    assert_eq!(stale.code(), ServiceErrorCode::Conflict);
    assert!(stale.to_string().contains("fresh_syntax="), "{stale}");

    let create = CreateFileRequest::new(
        "create-extra",
        project,
        ProjectRelativePath::new("src/nested/extra.rs").expect("create path"),
        "pub fn extra() -> usize { 3 }",
        mutation.generation,
    )
    .with_parent_creation(true);
    let created = services.create_file(&create).expect("create file");
    assert!(repo.join("src/nested/extra.rs").is_file());
    assert!(!created.changed_graph_ids.is_empty());
    let existing = services
        .create_file(&CreateFileRequest {
            operation_id: "create-extra-again".to_owned(),
            expected_generation: created.generation,
            ..create
        })
        .expect_err("existing destination must fail");
    assert_eq!(existing.code(), ServiceErrorCode::Conflict);
}
