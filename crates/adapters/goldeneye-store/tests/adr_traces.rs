use goldeneye_domain::{ProjectId, ProjectRecord};
use goldeneye_store::{
    AdrSection, CURRENT_SCHEMA_VERSION, RuntimeTrace, Store, parse_adr_sections,
    render_adr_sections,
};
use tempfile::TempDir;

fn registered_store(temp: &TempDir) -> (Store, ProjectId, std::path::PathBuf) {
    let database = temp.path().join("graph.sqlite3");
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("project root");
    let project = ProjectId::new("adr-traces").expect("project ID");
    let mut store = Store::open(&database).expect("store");
    store
        .register_project(
            &ProjectRecord::new(project.clone(), root.to_string_lossy()).expect("project record"),
        )
        .expect("register project");
    (store, project, database)
}

#[test]
fn adr_and_runtime_traces_are_durable_and_project_scoped() {
    let temp = TempDir::new().expect("temp dir");
    let (mut store, project, database) = registered_store(&temp);

    assert_eq!(CURRENT_SCHEMA_VERSION, 6);
    let schema = store.schema_info().expect("schema");
    assert!(schema.tables.contains("project_summaries"));
    assert!(schema.tables.contains("runtime_traces"));

    store
        .store_adr(&project, "## PURPOSE\nDurable context")
        .expect("store ADR");
    let adr = store
        .get_adr(&project)
        .expect("read ADR")
        .expect("ADR exists");
    assert_eq!(adr.project, project);
    assert_eq!(adr.content, "## PURPOSE\nDurable context");
    assert!(!adr.created_at.is_empty());
    assert!(!adr.updated_at.is_empty());

    let traces = [
        RuntimeTrace::new("crate::caller", "crate::callee", 2).expect("trace"),
        RuntimeTrace::new("crate::caller", "crate::callee", 3).expect("trace"),
        RuntimeTrace::new("crate::caller", "crate::other", 1).expect("trace"),
    ];
    assert_eq!(
        store
            .ingest_runtime_traces(&project, &traces)
            .expect("ingest traces"),
        3
    );
    drop(store);

    let reopened = Store::open_read_only(&database).expect("read-only reopen");
    assert_eq!(
        reopened
            .get_adr(&project)
            .expect("read ADR")
            .expect("ADR exists")
            .content,
        "## PURPOSE\nDurable context"
    );
    let persisted = reopened
        .list_runtime_traces(&project)
        .expect("runtime traces");
    assert_eq!(persisted.len(), 2);
    assert_eq!(persisted[0].caller, "crate::caller");
    assert_eq!(persisted[0].callee, "crate::callee");
    assert_eq!(persisted[0].count, 5);
    assert_eq!(persisted[1].callee, "crate::other");
    assert_eq!(persisted[1].count, 1);
}

#[test]
fn adr_parser_and_renderer_match_upstream_canonical_behavior() {
    let parsed =
        parse_adr_sections("preamble\n## PURPOSE\nWhy\n## CUSTOM\nStill why\n\n## STACK\nRust\n");
    assert_eq!(
        parsed,
        vec![
            AdrSection::new("PURPOSE", "Why\n## CUSTOM\nStill why"),
            AdrSection::new("STACK", "Rust"),
        ]
    );
    assert_eq!(
        render_adr_sections(&[
            AdrSection::new("ZEBRA", "Z"),
            AdrSection::new("STACK", "Rust"),
            AdrSection::new("PURPOSE", "Why"),
            AdrSection::new("ALPHA", "A"),
        ]),
        "## PURPOSE\nWhy\n\n## STACK\nRust\n\n## ALPHA\nA\n\n## ZEBRA\nZ"
    );
}

#[test]
fn section_updates_merge_in_canonical_order_and_enforce_upstream_limit() {
    let temp = TempDir::new().expect("temp dir");
    let (mut store, project, _) = registered_store(&temp);
    store
        .store_adr(&project, "## PURPOSE\nOld\n\n## STACK\nC")
        .expect("seed ADR");

    let updated = store
        .update_adr_sections(
            &project,
            &[
                AdrSection::new("STACK", "Rust"),
                AdrSection::new("CUSTOM", "Extension"),
            ],
        )
        .expect("update sections");
    assert_eq!(
        updated.content,
        "## PURPOSE\nOld\n\n## STACK\nRust\n\n## CUSTOM\nExtension"
    );

    let too_large = "x".repeat(8_001);
    assert!(
        store
            .update_adr_sections(&project, &[AdrSection::new("PURPOSE", too_large)])
            .expect_err("oversized merged ADR")
            .to_string()
            .contains("merged ADR exceeds 8000 chars")
    );
}
