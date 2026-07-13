use std::collections::BTreeSet;

use goldeneye_domain::{
    ByteSpan, ContentHash, EdgeKind, FileId, FileRecord, Generation, GraphEdge, GraphNode, NodeId,
    NodeLabel, ProjectId, ProjectRecord, ProjectRelativePath, QualifiedName, SourcePoint,
    SourceSpan,
};
use goldeneye_store::{CURRENT_SCHEMA_VERSION, Store, StoreError};
use rusqlite::Connection;
use tempfile::TempDir;

fn project(name: &str, root: &str) -> ProjectRecord {
    ProjectRecord::new(ProjectId::new(name).expect("valid project ID"), root)
        .expect("valid project")
}

fn rel(path: &str) -> ProjectRelativePath {
    ProjectRelativePath::new(path).expect("valid relative path")
}

fn file(project: &ProjectId, path: &str, generation: Generation, bytes: &[u8]) -> FileRecord {
    FileRecord::new(
        FileId::new(project.clone(), rel(path)),
        ContentHash::of(bytes),
        generation,
        123,
        u64::try_from(bytes.len()).expect("fixture length fits u64"),
    )
}

fn node(
    project: &ProjectId,
    id: &str,
    label: &str,
    name: &str,
    qualified_name: &str,
    path: &str,
    generation: Generation,
) -> GraphNode {
    let span = SourceSpan::new(
        ByteSpan::new(0, 4).expect("valid byte span"),
        SourcePoint::new(0, 0),
        SourcePoint::new(0, 4),
    )
    .expect("valid source span");
    GraphNode::new(
        project.clone(),
        NodeId::new(id).expect("valid node ID"),
        NodeLabel::new(label).expect("valid label"),
        name,
        QualifiedName::new(qualified_name).expect("valid qualified name"),
        Some(rel(path)),
        Some(span),
        generation,
    )
    .expect("valid node")
}

fn edge(
    project: &ProjectId,
    source: &str,
    target: &str,
    kind: &str,
    generation: Generation,
) -> GraphEdge {
    GraphEdge::new(
        project.clone(),
        NodeId::new(source).expect("valid source ID"),
        NodeId::new(target).expect("valid target ID"),
        EdgeKind::new(kind).expect("valid edge kind"),
        generation,
    )
}

fn open_file_store(temp: &TempDir) -> (Store, std::path::PathBuf) {
    let path = temp.path().join("graph.sqlite3");
    let store = Store::open(&path).expect("open store");
    (store, path)
}

#[test]
fn migrations_are_versioned_idempotent_and_introspectable() {
    let temp = TempDir::new().expect("temp dir");
    let (store, path) = open_file_store(&temp);
    let schema = store.schema_info().expect("schema info");

    assert_eq!(schema.version, CURRENT_SCHEMA_VERSION);
    assert!(schema.fts5_enabled);
    for table in [
        "schema_migrations",
        "projects",
        "files",
        "nodes",
        "edges",
        "nodes_fts",
    ] {
        assert!(schema.tables.contains(table), "missing table {table}");
    }
    drop(store);

    let reopened = Store::open(&path).expect("idempotent reopen");
    assert_eq!(
        reopened.schema_info().expect("schema").version,
        CURRENT_SCHEMA_VERSION
    );
}

#[test]
fn newer_schema_is_rejected_without_partial_migration() {
    let temp = TempDir::new().expect("temp dir");
    let path = temp.path().join("future.sqlite3");
    let connection = Connection::open(&path).expect("seed database");
    connection
        .execute_batch(
            "CREATE TABLE schema_migrations (\
                 version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL\
             );\
             INSERT INTO schema_migrations VALUES (999, CURRENT_TIMESTAMP);\
             PRAGMA user_version = 999;",
        )
        .expect("seed future schema");
    drop(connection);

    assert!(matches!(
        Store::open(&path),
        Err(StoreError::SchemaTooNew {
            actual: 999,
            supported: CURRENT_SCHEMA_VERSION,
        })
    ));
    let connection = Connection::open(&path).expect("inspect database");
    let projects_exist: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name = 'projects')",
            [],
            |row| row.get(0),
        )
        .expect("inspect schema");
    assert!(!projects_exist);
}

#[test]
fn file_store_enables_durable_pragmas() {
    let temp = TempDir::new().expect("temp dir");
    let (store, _) = open_file_store(&temp);
    let settings = store.connection_settings().expect("settings");

    assert!(settings.foreign_keys);
    assert_eq!(settings.journal_mode, "wal");
    assert_eq!(settings.synchronous, 1);
    assert_eq!(settings.busy_timeout_ms, 10_000);
    assert!(!settings.query_only);
}

#[test]
fn project_file_identity_and_generations_survive_reopen() {
    let temp = TempDir::new().expect("temp dir");
    let (mut store, path) = open_file_store(&temp);
    let project = project("Goldeneye", "D:/Dév/金眼");
    store.register_project(&project).expect("register project");
    let generation = store
        .begin_generation(&project.id)
        .expect("begin generation");
    assert_eq!(generation, Generation::new(1));

    let upper = file(&project.id, "Src/Δelta.rs", generation, b"UPPER");
    let lower = file(&project.id, "src/Δelta.rs", generation, b"lower");
    store.upsert_file(&upper).expect("upper-case file");
    store.upsert_file(&lower).expect("lower-case file");
    drop(store);

    let reopened = Store::open(&path).expect("reopen");
    let mut indexed_project = project;
    indexed_project.generation = generation;
    assert_eq!(
        reopened.get_project(&indexed_project.id).expect("project"),
        Some(indexed_project)
    );
    assert_eq!(
        reopened.get_file(&upper.id).expect("upper file"),
        Some(upper)
    );
    assert_eq!(
        reopened.get_file(&lower.id).expect("lower file"),
        Some(lower)
    );
}

#[test]
fn replacing_file_graph_is_atomic_and_deterministic() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    store.register_project(&project).expect("project");
    let generation = store.begin_generation(&project.id).expect("generation");
    let file = file(&project.id, "src/lib.rs", generation, b"fn alpha() {}");
    let alpha = node(
        &project.id,
        "n-alpha",
        "Function",
        "alpha",
        "p.alpha",
        "src/lib.rs",
        generation,
    );
    let beta = node(
        &project.id,
        "n-beta",
        "Function",
        "beta",
        "p.beta",
        "src/lib.rs",
        generation,
    );
    let calls = edge(&project.id, "n-alpha", "n-beta", "CALLS", generation);

    let first = store
        .replace_file_graph(
            &file,
            &[beta.clone(), alpha.clone()],
            std::slice::from_ref(&calls),
        )
        .expect("first replacement");
    let first_nodes = store.nodes_for_file(&file.id).expect("nodes");
    let first_edges = store.edges_from(&project.id, &alpha.id).expect("edges");
    let second = store
        .replace_file_graph(
            &file,
            &[alpha.clone(), beta.clone()],
            std::slice::from_ref(&calls),
        )
        .expect("second replacement");

    assert_eq!(first, second);
    assert_eq!(store.nodes_for_file(&file.id).expect("nodes"), first_nodes);
    assert_eq!(
        store.edges_from(&project.id, &alpha.id).expect("edges"),
        first_edges
    );
    assert_eq!(first_nodes, vec![alpha, beta]);
    assert_eq!(first_edges, vec![calls]);
}

#[test]
fn failed_replacement_rolls_back_file_and_graph() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    store.register_project(&project).expect("project");
    let generation = store.begin_generation(&project.id).expect("generation");
    let original_file = file(&project.id, "src/lib.rs", generation, b"old");
    let original = node(
        &project.id,
        "old",
        "Function",
        "old",
        "p.old",
        "src/lib.rs",
        generation,
    );
    store
        .replace_file_graph(&original_file, std::slice::from_ref(&original), &[])
        .expect("seed graph");

    let replacement_file = file(&project.id, "src/lib.rs", generation, b"new");
    let replacement = node(
        &project.id,
        "new",
        "Function",
        "new",
        "p.new",
        "src/lib.rs",
        generation,
    );
    let dangling = edge(&project.id, "new", "missing", "CALLS", generation);
    assert!(matches!(
        store.replace_file_graph(
            &replacement_file,
            std::slice::from_ref(&replacement),
            std::slice::from_ref(&dangling),
        ),
        Err(StoreError::MissingNode { node_id }) if node_id == NodeId::new("missing").expect("ID")
    ));

    assert_eq!(
        store.get_file(&original_file.id).expect("file"),
        Some(original_file)
    );
    assert_eq!(
        store.nodes_for_file(&replacement_file.id).expect("nodes"),
        vec![original]
    );
}

#[test]
fn constraints_reject_cross_project_and_stale_generation_writes() {
    let mut store = Store::open_in_memory().expect("memory store");
    let left = project("left", "/left");
    let right = project("right", "/right");
    store.register_project(&left).expect("left");
    store.register_project(&right).expect("right");
    let current = store.begin_generation(&left.id).expect("generation");
    let stale = Generation::new(0);
    let stale_file = file(&left.id, "lib.rs", stale, b"stale");
    assert!(matches!(
        store.upsert_file(&stale_file),
        Err(StoreError::GenerationMismatch { expected, actual })
            if expected == current && actual == stale
    ));

    let valid_file = file(&left.id, "lib.rs", current, b"ok");
    let foreign = node(
        &right.id,
        "foreign",
        "Function",
        "foreign",
        "right.foreign",
        "lib.rs",
        current,
    );
    assert!(matches!(
        store.replace_file_graph(&valid_file, &[foreign], &[]),
        Err(StoreError::ProjectMismatch { .. })
    ));
}

#[test]
fn complete_project_replacement_registers_and_swaps_one_generation_atomically() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/canonical/repo");
    let pending = Generation::new(0);
    let first_file = file(&project.id, "src/old.rs", pending, b"old");
    let first_node = node(
        &project.id,
        "old",
        "Function",
        "old",
        "p.src.old.old",
        "src/old.rs",
        pending,
    );

    let first = store
        .replace_project_graph(
            &project,
            vec![first_file.clone()],
            vec![first_node.clone()],
            Vec::new(),
        )
        .expect("initial replacement");
    assert_eq!(first.generation, Generation::new(1));
    assert_eq!((first.files, first.nodes, first.edges), (1, 1, 0));
    assert_eq!(
        store.get_project(&project.id).expect("project"),
        Some(ProjectRecord {
            generation: Generation::new(1),
            ..project.clone()
        })
    );

    let second_file = file(&project.id, "src/new.rs", pending, b"new");
    let second_node = node(
        &project.id,
        "new",
        "Function",
        "new",
        "p.src.new.new",
        "src/new.rs",
        pending,
    );
    let second = store
        .replace_project_graph(
            &project,
            vec![second_file.clone()],
            vec![second_node.clone()],
            Vec::new(),
        )
        .expect("second replacement");
    assert_eq!(second.generation, Generation::new(2));
    assert_eq!(store.get_file(&first_file.id).expect("old file"), None);
    assert_eq!(
        store.nodes_for_file(&second_file.id).expect("new nodes"),
        vec![GraphNode {
            generation: Generation::new(2),
            ..second_node
        }]
    );
    assert_eq!(
        store.counts(&project.id).expect("counts"),
        goldeneye_store::GraphCounts {
            files: 1,
            nodes: 1,
            edges: 0,
        }
    );
}

#[test]
fn failed_complete_project_replacement_rolls_back_registration_generation_and_graph() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    let pending = Generation::new(0);
    let original_file = file(&project.id, "src/lib.rs", pending, b"old");
    let original_node = node(
        &project.id,
        "old",
        "Function",
        "old",
        "p.old",
        "src/lib.rs",
        pending,
    );
    store
        .replace_project_graph(
            &project,
            vec![original_file.clone()],
            vec![original_node.clone()],
            Vec::new(),
        )
        .expect("seed graph");

    let replacement_file = file(&project.id, "src/lib.rs", pending, b"new");
    let replacement_node = node(
        &project.id,
        "new",
        "Function",
        "new",
        "p.new",
        "src/lib.rs",
        pending,
    );
    let dangling = edge(&project.id, "new", "missing", "CALLS", pending);
    assert!(matches!(
        store.replace_project_graph(
            &project,
            vec![replacement_file],
            vec![replacement_node],
            vec![dangling],
        ),
        Err(StoreError::MissingNode { node_id })
            if node_id == NodeId::new("missing").expect("node ID")
    ));

    assert_eq!(
        store
            .get_project(&project.id)
            .expect("project")
            .map(|value| value.generation),
        Some(Generation::new(1))
    );
    assert_eq!(
        store.get_file(&original_file.id).expect("file"),
        Some(FileRecord {
            generation: Generation::new(1),
            ..original_file.clone()
        })
    );
    assert_eq!(
        store.nodes_for_file(&original_file.id).expect("nodes"),
        vec![GraphNode {
            generation: Generation::new(1),
            ..original_node
        }]
    );

    let unregistered =
        ProjectRecord::new(ProjectId::new("bad").expect("project ID"), "/bad").expect("project");
    let foreign = node(
        &project.id,
        "foreign",
        "Function",
        "foreign",
        "bad.foreign",
        "bad.rs",
        pending,
    );
    assert!(matches!(
        store.replace_project_graph(&unregistered, Vec::new(), vec![foreign], Vec::new()),
        Err(StoreError::ProjectMismatch { .. })
    ));
    assert_eq!(
        store.get_project(&unregistered.id).expect("bad project"),
        None
    );
}

#[test]
fn reconcile_deletes_unseen_files_and_touches_retained_generation() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    store.register_project(&project).expect("project");
    let first = store
        .begin_generation(&project.id)
        .expect("first generation");
    let keep = file(&project.id, "keep.rs", first, b"keep");
    let remove = file(&project.id, "remove.rs", first, b"remove");
    store.upsert_file(&keep).expect("keep");
    store.upsert_file(&remove).expect("remove");

    let second = store
        .begin_generation(&project.id)
        .expect("second generation");
    let outcome = store
        .reconcile_project(&project.id, second, &BTreeSet::from([rel("keep.rs")]))
        .expect("reconcile");

    assert_eq!(outcome.removed_files, 1);
    assert_eq!(store.get_file(&remove.id).expect("removed"), None);
    let kept = store.get_file(&keep.id).expect("kept").expect("present");
    assert_eq!(kept.generation, second);
}

#[test]
fn fts_search_handles_unicode_and_case_insensitive_terms() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    store.register_project(&project).expect("project");
    let generation = store.begin_generation(&project.id).expect("generation");
    let file = file(&project.id, "src/unicode.rs", generation, b"unicode");
    let delta = node(
        &project.id,
        "delta",
        "Function",
        "ΔeltaHandler",
        "pkg.ΔeltaHandler",
        "src/unicode.rs",
        generation,
    );
    store
        .replace_file_graph(&file, std::slice::from_ref(&delta), &[])
        .expect("replace");

    let lower = store
        .search_nodes(&project.id, "δeltahandler", 10)
        .expect("search");
    assert_eq!(lower.len(), 1);
    assert_eq!(lower[0].node, delta);
}

#[test]
fn graph_replacement_removes_stale_fts_terms() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    store.register_project(&project).expect("project");
    let generation = store.begin_generation(&project.id).expect("generation");
    let original_file = file(&project.id, "src/lib.rs", generation, b"old");
    let original = node(
        &project.id,
        "old",
        "Function",
        "obsolete_token",
        "p.obsolete_token",
        "src/lib.rs",
        generation,
    );
    store
        .replace_file_graph(&original_file, &[original], &[])
        .expect("seed graph");

    let replacement_file = file(&project.id, "src/lib.rs", generation, b"new");
    let replacement = node(
        &project.id,
        "new",
        "Function",
        "current_token",
        "p.current_token",
        "src/lib.rs",
        generation,
    );
    store
        .replace_file_graph(&replacement_file, &[replacement], &[])
        .expect("replace graph");

    assert!(
        store
            .search_nodes(&project.id, "obsolete_token", 10)
            .expect("search old")
            .is_empty()
    );
    assert_eq!(
        store
            .search_nodes(&project.id, "current_token", 10)
            .expect("search new")
            .len(),
        1
    );
}

#[test]
fn edge_discriminator_preserves_distinct_named_imports() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    store.register_project(&project).expect("project");
    let generation = store.begin_generation(&project.id).expect("generation");
    let file = file(&project.id, "src/lib.ts", generation, b"imports");
    let source = node(
        &project.id,
        "source",
        "File",
        "lib",
        "p.lib",
        "src/lib.ts",
        generation,
    );
    let target = node(
        &project.id,
        "target",
        "Module",
        "dep",
        "dep",
        "src/lib.ts",
        generation,
    );
    let first = edge(&project.id, "source", "target", "IMPORTS", generation)
        .with_discriminator("First")
        .expect("discriminator");
    let second = edge(&project.id, "source", "target", "IMPORTS", generation)
        .with_discriminator("Second")
        .expect("discriminator");

    store
        .replace_file_graph(
            &file,
            &[source.clone(), target],
            &[second.clone(), first.clone()],
        )
        .expect("replace");
    assert_eq!(
        store.edges_from(&project.id, &source.id).expect("edges"),
        vec![first, second]
    );
}

#[test]
fn read_only_open_never_creates_or_mutates_database() {
    let temp = TempDir::new().expect("temp dir");
    let missing = temp.path().join("missing.sqlite3");
    assert!(matches!(
        Store::open_read_only(&missing),
        Err(StoreError::DatabaseNotFound(_))
    ));
    assert!(!missing.exists());

    let (mut writable, path) = open_file_store(&temp);
    let project = project("p", "/repo");
    writable.register_project(&project).expect("project");
    drop(writable);

    let query = Store::open_read_only(&path).expect("read-only open");
    assert!(query.connection_settings().expect("settings").query_only);
    assert_eq!(
        query.get_project(&project.id).expect("project"),
        Some(project)
    );
}

#[test]
fn project_delete_cascades_files_nodes_edges_and_fts_rows() {
    let mut store = Store::open_in_memory().expect("memory store");
    let project = project("p", "/repo");
    store.register_project(&project).expect("project");
    let generation = store.begin_generation(&project.id).expect("generation");
    let file = file(&project.id, "src/lib.rs", generation, b"code");
    let node = node(
        &project.id,
        "node",
        "Function",
        "needle",
        "p.needle",
        "src/lib.rs",
        generation,
    );
    store
        .replace_file_graph(&file, &[node], &[])
        .expect("replace");

    assert!(store.delete_project(&project.id).expect("delete"));
    assert_eq!(store.counts(&project.id).expect("counts").files, 0);
    assert!(
        store
            .search_nodes(&project.id, "needle", 10)
            .expect("search")
            .is_empty()
    );
}
