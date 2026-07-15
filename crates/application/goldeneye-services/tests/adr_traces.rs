use std::fs;
use std::sync::Arc;

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_domain::ProjectRecord;
use goldeneye_git::GitCommandRepository;
use goldeneye_services::{
    IngestTracesRequest, MAX_PERSISTED_TRACE_BATCH, ManageAdrRequest, ProjectId, ServiceConfig,
    ServiceDependencies, ServiceErrorCode, Services,
};
use goldeneye_store::Store;
use serde_json::json;
use tempfile::TempDir;

fn service_dependencies() -> ServiceDependencies {
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
    )
}

fn registered_services(
    temp: &TempDir,
) -> (Services, ProjectId, std::path::PathBuf, std::path::PathBuf) {
    let database = temp.path().join("graph.sqlite3");
    let root = temp.path().join("project");
    fs::create_dir(&root).expect("project root");
    let project = ProjectId::new("adr-traces").expect("project ID");
    let mut store = Store::open(&database).expect("store");
    store
        .register_project(
            &ProjectRecord::new(project.clone(), root.to_string_lossy()).expect("project record"),
        )
        .expect("register project");
    drop(store);
    let services = Services::new(
        ServiceConfig::new(&database, &root).with_allowed_root(temp.path()),
        service_dependencies(),
    );
    (services, project, database, root)
}

#[test]
fn manage_adr_migrates_legacy_content_and_matches_upstream_modes() {
    let temp = TempDir::new().expect("temp dir");
    let (services, project, database, root) = registered_services(&temp);
    fs::create_dir(root.join(".codebase-memory")).expect("legacy directory");
    fs::write(
        root.join(".codebase-memory/adr.md"),
        "# Legacy ADR\n## PURPOSE\nImported",
    )
    .expect("legacy ADR");

    let migrated = services
        .manage_adr(&ManageAdrRequest::new(&project))
        .expect("migrate legacy ADR");
    assert_eq!(
        migrated.content.as_deref(),
        Some("# Legacy ADR\n## PURPOSE\nImported")
    );
    assert!(migrated.status.is_none());
    fs::remove_file(root.join(".codebase-memory/adr.md")).expect("remove legacy ADR");
    assert_eq!(
        Store::open_read_only(&database)
            .expect("store")
            .get_adr(&project)
            .expect("read ADR")
            .expect("migrated ADR")
            .content,
        "# Legacy ADR\n## PURPOSE\nImported"
    );

    let updated = services
        .manage_adr(&ManageAdrRequest {
            project: project.as_str().to_owned(),
            mode: Some("update".to_owned()),
            content: Some("# Architecture\n## PURPOSE\nCurrent\n### Detail".to_owned()),
            sections: Vec::new(),
        })
        .expect("update ADR");
    assert_eq!(updated.status.as_deref(), Some("updated"));
    assert!(updated.content.is_none());

    let by_absolute_path = services
        .manage_adr(&ManageAdrRequest {
            project: root.to_string_lossy().into_owned(),
            mode: Some("get".to_owned()),
            content: None,
            sections: Vec::new(),
        })
        .expect("registered absolute root resolves to project");
    assert_eq!(
        by_absolute_path.content.as_deref(),
        Some("# Architecture\n## PURPOSE\nCurrent\n### Detail")
    );

    let sections = services
        .manage_adr(&ManageAdrRequest {
            project: project.as_str().to_owned(),
            mode: Some("sections".to_owned()),
            content: None,
            sections: vec!["ignored upstream input".to_owned()],
        })
        .expect("list sections");
    assert_eq!(
        sections.sections,
        Some(vec![
            "# Architecture".to_owned(),
            "## PURPOSE".to_owned(),
            "### Detail".to_owned(),
        ])
    );

    let fallback_get = services
        .manage_adr(&ManageAdrRequest {
            project: project.as_str().to_owned(),
            mode: Some("unknown".to_owned()),
            content: None,
            sections: Vec::new(),
        })
        .expect("unknown mode falls back to get");
    assert_eq!(
        fallback_get.content.as_deref(),
        Some("# Architecture\n## PURPOSE\nCurrent\n### Detail")
    );
}

#[test]
fn manage_adr_returns_compact_no_adr_hint() {
    let temp = TempDir::new().expect("temp dir");
    let (services, project, _, _) = registered_services(&temp);

    let result = services
        .manage_adr(&ManageAdrRequest::new(&project))
        .expect("empty ADR");
    assert_eq!(result.content.as_deref(), Some(""));
    assert_eq!(result.status.as_deref(), Some("no_adr"));
    assert!(
        result
            .adr_hint
            .as_deref()
            .is_some_and(|hint| hint.contains("PURPOSE, STACK, ARCHITECTURE"))
    );
}

#[test]
fn trace_ingestion_is_bounded_skips_partial_items_and_persists_valid_edges() {
    let temp = TempDir::new().expect("temp dir");
    let (services, project, database, _) = registered_services(&temp);
    assert_eq!(MAX_PERSISTED_TRACE_BATCH, 1_024);

    let traces = (0..(MAX_PERSISTED_TRACE_BATCH + 10))
        .map(|index| {
            json!({
                "caller": format!("caller::{index}"),
                "callee": "callee::shared",
                "count": 2
            })
        })
        .collect::<Vec<_>>();
    let result = services
        .ingest_traces(&IngestTracesRequest {
            project: project.clone(),
            traces,
        })
        .expect("ingest bounded batch");
    assert_eq!(result.status, "accepted");
    assert_eq!(result.traces_received, MAX_PERSISTED_TRACE_BATCH + 10);
    assert_eq!(
        result.note,
        "Runtime edge creation from traces not yet implemented"
    );

    let partial = services
        .ingest_traces(&IngestTracesRequest {
            project: project.clone(),
            traces: vec![
                json!({}),
                json!({"caller": "only-caller"}),
                json!({"caller": "valid", "callee": "edge"}),
                json!({"caller": "valid", "callee": "edge", "count": 4}),
                json!({"caller": "", "callee": "ignored"}),
            ],
        })
        .expect("partial traces remain accepted");
    assert_eq!(partial.traces_received, 5);

    let persisted = Store::open_read_only(&database)
        .expect("store")
        .list_runtime_traces(&project)
        .expect("runtime traces");
    assert_eq!(persisted.len(), MAX_PERSISTED_TRACE_BATCH + 1);
    let merged = persisted
        .iter()
        .find(|trace| trace.caller == "valid")
        .expect("merged trace");
    assert_eq!(merged.count, 5);
}

#[test]
fn adr_and_trace_writes_enforce_registered_project_path_policy() {
    let temp = TempDir::new().expect("temp dir");
    let database = temp.path().join("graph.sqlite3");
    let allowed = temp.path().join("allowed");
    let outside = temp.path().join("outside");
    fs::create_dir(&allowed).expect("allowed root");
    fs::create_dir(&outside).expect("outside root");
    let project = ProjectId::new("outside-project").expect("project ID");
    let mut store = Store::open(&database).expect("store");
    store
        .register_project(
            &ProjectRecord::new(project.clone(), outside.to_string_lossy())
                .expect("project record"),
        )
        .expect("register outside project");
    drop(store);
    let services = Services::new(
        ServiceConfig::new(&database, &allowed).with_allowed_root(&allowed),
        service_dependencies(),
    );

    assert_eq!(
        services
            .manage_adr(&ManageAdrRequest::new(&project))
            .expect_err("ADR path policy")
            .code(),
        ServiceErrorCode::Forbidden
    );
    assert_eq!(
        services
            .ingest_traces(&IngestTracesRequest {
                project,
                traces: vec![json!({"caller": "a", "callee": "b"})],
            })
            .expect_err("trace path policy")
            .code(),
        ServiceErrorCode::Forbidden
    );
}
