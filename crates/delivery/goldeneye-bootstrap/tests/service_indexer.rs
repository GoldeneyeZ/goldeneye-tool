use std::fs;
use std::sync::Arc;

use goldeneye_bootstrap::{BootstrapRuntime, ServiceIndexer, service_dependencies};
use goldeneye_services::{
    ArchitectureRequest, IndexRepositoryMode, IndexRepositoryRequest, ProjectId, ServiceConfig,
    ServiceErrorCode, Services,
};
use goldeneye_watcher::{IndexDisposition, Indexer};

#[test]
fn prune_is_a_no_op_before_project_validation_when_database_is_missing() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let services = Services::new(
        ServiceConfig::new(temp.path().join("missing.db"), temp.path()),
        service_dependencies(),
    );
    let indexer = ServiceIndexer::new(services);

    indexer
        .prune("", temp.path())
        .expect("missing database precedes project validation");
}

#[test]
fn dropping_runtime_stops_its_watcher() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let runtime = BootstrapRuntime::from_config(ServiceConfig::new(
        temp.path().join("missing.db"),
        temp.path(),
    ));
    let watcher = Arc::clone(runtime.watcher());
    assert!(!watcher.is_stopped());

    drop(runtime);

    assert!(watcher.is_stopped());
}

#[test]
fn reindex_and_prune_are_visible_through_the_original_services_cache() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let root = temp.path().join("repo");
    fs::create_dir(&root).expect("repository directory");
    fs::write(root.join("lib.rs"), "fn first() {}\n").expect("initial source");
    let database = temp.path().join("graph.sqlite3");
    let services = Services::new(ServiceConfig::new(&database, &root), service_dependencies());
    let request = IndexRepositoryRequest {
        repo_path: root.clone(),
        name: Some("demo".to_owned()),
        mode: IndexRepositoryMode::Fast,
        persistence: false,
    };
    services.index_repository(&request).expect("initial index");
    let project = ProjectId::new("demo").expect("project ID");
    let before = services
        .get_architecture(&ArchitectureRequest::new(project.clone()))
        .expect("warm architecture cache");
    let runtime = BootstrapRuntime::new(services.clone());
    let projects = runtime.watcher().projects().expect("seeded projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].project, "demo");
    let indexer = ServiceIndexer::new(runtime.services().clone());

    fs::write(root.join("lib.rs"), "fn first() {}\nfn second() {}\n").expect("updated source");
    assert_eq!(
        indexer.index("demo", &root).expect("background reindex"),
        IndexDisposition::Indexed
    );
    let after = services
        .get_architecture(&ArchitectureRequest::new(project.clone()))
        .expect("architecture after shared reindex");
    assert!(after.total_nodes > before.total_nodes);

    indexer.prune("demo", &root).expect("background prune");
    let error = services
        .get_architecture(&ArchitectureRequest::new(project))
        .expect_err("shared prune invalidates architecture cache");
    assert_eq!(error.code(), ServiceErrorCode::NotFound);
}
