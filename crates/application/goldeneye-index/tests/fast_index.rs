use std::collections::BTreeSet;
use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;

use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_domain::{EdgeKind, Generation, NodeId, ProjectRelativePath};
use goldeneye_index::{
    CancellationToken, FileRefreshStatus, IndexError, IndexOptions, IndexService, IndexStatus,
};
use goldeneye_store::Store;
use goldeneye_syntax::CoreGrammarProvider;
use tempfile::TempDir;

type NodeSnapshot = Vec<(String, String, String, Option<String>)>;
type EdgeSnapshot = Vec<(String, String, String)>;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("fixture parent")).expect("create fixture parent");
    fs::write(path, source).expect("write fixture");
}

fn remove(root: &Path, path: &str) {
    fs::remove_file(root.join(path)).expect("remove fixture");
}

fn service(options: IndexOptions) -> IndexService<CoreGrammarProvider, Store> {
    IndexService::new(
        Store::open_in_memory().expect("memory store"),
        CoreGrammarProvider,
        options,
        FileSystemDiscovery,
    )
}

fn nodes_for(
    service: &IndexService<CoreGrammarProvider, Store>,
    project: &goldeneye_domain::ProjectId,
    path: &str,
) -> Vec<goldeneye_domain::GraphNode> {
    let file = goldeneye_domain::FileId::new(
        project.clone(),
        ProjectRelativePath::new(path).expect("relative path"),
    );
    service
        .repository()
        .nodes_for_file(&file)
        .expect("nodes for fixture")
}

fn graph_snapshot(
    service: &IndexService<CoreGrammarProvider, Store>,
    project: &goldeneye_domain::ProjectId,
) -> (NodeSnapshot, EdgeSnapshot) {
    let mut nodes = Vec::new();
    let mut edges = BTreeSet::new();
    for file in service.repository().list_files(project).expect("files") {
        for node in service
            .repository()
            .nodes_for_file(&file.id)
            .expect("nodes")
        {
            for edge in service
                .repository()
                .edges_from(project, &node.id)
                .expect("edges")
            {
                edges.insert((
                    edge.source.as_str().to_owned(),
                    edge.target.as_str().to_owned(),
                    edge.kind.as_str().to_owned(),
                ));
            }
            nodes.push((
                node.id.as_str().to_owned(),
                node.label.as_str().to_owned(),
                node.qualified_name.as_str().to_owned(),
                node.file_path.map(|path| path.as_str().to_owned()),
            ));
        }
    }
    nodes.sort();
    (nodes, edges.into_iter().collect())
}

fn seed_multilang(root: &Path) {
    write(
        root,
        "src/lib.rs",
        "use crate::util::helper;\nstruct User { name: String }\nimpl User { fn greet(&self) { helper(); } }\nfn helper() {}\n",
    );
    write(
        root,
        "pkg/café.py",
        "from pkg.util import helper\nclass Engine:\n    field = 1\n    def run(self):\n        helper()\ndef helper():\n    value = 1\n",
    );
    write(
        root,
        "web/app.js",
        "import { helper } from './util.js';\nclass Widget { field = 1; render() { helper(); } }\nfunction helper() {}\nconst answer = 42;\n",
    );
    write(
        root,
        "web/MixedCase.ts",
        "import { helper } from './util';\ninterface Item { name: string }\nclass Box { value: number = 1; run(): void { helper(); } }\nfunction helper(): void {}\n",
    );
    write(
        root,
        "cmd/main.go",
        "package main\nimport \"fmt\"\ntype User struct { Name string }\nfunc (u User) Greet() { helper() }\nfunc helper() {}\nfunc main() { value := 1; fmt.Println(value) }\n",
    );
    write(root, "ignored.py", "def should_not_exist():\n    pass\n");
    write(root, ".gitignore", "ignored.py\nlinked.py\n");
    let link = root.join("linked.py");
    #[cfg(unix)]
    let _ = std::os::unix::fs::symlink(root.join("pkg/café.py"), link);
    #[cfg(windows)]
    let _ = std::os::windows::fs::symlink_file(root.join("pkg/café.py"), link);
}

#[test]
fn unsupported_discovered_languages_do_not_abort_core_indexing() {
    let temp = TempDir::new().expect("temp repo");
    write(
        temp.path(),
        "Cargo.toml",
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    );
    write(
        temp.path(),
        "src/lib.rs",
        "pub fn helper() -> usize { 1 }\n",
    );
    let mut index = service(IndexOptions::default());

    let result = index
        .index_repository(temp.path())
        .expect("unsupported TOML is skipped");

    assert_eq!(result.status, IndexStatus::Indexed);
    assert_eq!(result.discovered_files, 1);
    assert_eq!(result.new_files, 1);
    assert_eq!(
        index
            .repository()
            .list_files(&result.project.id)
            .expect("stored files")
            .len(),
        1
    );
}

#[test]
fn initial_index_extracts_stable_multilanguage_graph() {
    let temp = TempDir::new().expect("temp repo");
    seed_multilang(temp.path());
    let options = IndexOptions {
        max_workers: NonZeroUsize::new(4).expect("workers"),
        ..IndexOptions::default()
    };
    let mut index = service(options);

    let first = index.index_repository(temp.path()).expect("initial index");
    assert_eq!(first.status, IndexStatus::Indexed);
    assert_eq!(first.new_files, 5);
    assert_eq!(first.changed_files, 0);
    assert_eq!(first.deleted_files, 0);
    assert!(first.diagnostics.is_empty());
    assert_eq!(
        first.project.root_path,
        goldeneye_index::canonical_root_string(temp.path()).expect("canonical root")
    );

    let (nodes, edges) = graph_snapshot(&index, &first.project.id);
    let labels = nodes
        .iter()
        .map(|(_, label, _, _)| label.as_str())
        .collect::<BTreeSet<_>>();
    for expected in [
        "File",
        "Module",
        "Struct",
        "Class",
        "Interface",
        "Function",
        "Method",
        "Field",
        "Variable",
        "Import",
    ] {
        assert!(labels.contains(expected), "missing {expected}: {labels:?}");
    }
    assert!(nodes.iter().any(
        |(_, _, qualified_name, path)| qualified_name.contains("café")
            && path.as_deref() == Some("pkg/café.py")
    ));
    assert!(nodes.iter().any(
        |(_, _, qualified_name, path)| qualified_name.contains("MixedCase")
            && path.as_deref() == Some("web/MixedCase.ts")
    ));
    assert!(edges.iter().any(|(_, _, kind)| kind == "DEFINES"));
    assert!(edges.iter().any(|(_, _, kind)| kind == "CALLS"));

    let second = index.index_repository(temp.path()).expect("stable repeat");
    assert_eq!(second.status, IndexStatus::Unchanged);
    assert_eq!(second.project.generation, first.project.generation);
    assert_eq!(second.unchanged_files, 5);
    assert_eq!(graph_snapshot(&index, &first.project.id), (nodes, edges));

    let serial_options = IndexOptions {
        max_workers: NonZeroUsize::new(1).expect("worker"),
        ..IndexOptions::default()
    };
    let mut serial = service(serial_options);
    let serial_result = serial.index_repository(temp.path()).expect("serial index");
    assert_eq!(
        graph_snapshot(&serial, &serial_result.project.id),
        graph_snapshot(&index, &first.project.id)
    );
}

#[test]
fn incremental_index_reconciles_changed_new_and_deleted_files() {
    let temp = TempDir::new().expect("temp repo");
    write(temp.path(), "src/change.rs", "fn old_name() {}\n");
    write(temp.path(), "pkg/delete.py", "def removed():\n    pass\n");
    write(temp.path(), "web/keep.js", "function stable() {}\n");
    let mut index = service(IndexOptions::default());
    let first = index.index_repository(temp.path()).expect("initial");
    let stable_ids = nodes_for(&index, &first.project.id, "web/keep.js")
        .into_iter()
        .map(|node| node.id)
        .collect::<Vec<_>>();

    write(temp.path(), "src/change.rs", "fn new_name() {}\n");
    write(temp.path(), "cmd/new.go", "package demo\nfunc Added() {}\n");
    remove(temp.path(), "pkg/delete.py");
    let second = index.index_repository(temp.path()).expect("incremental");

    assert_eq!(second.status, IndexStatus::Indexed);
    assert_eq!(second.changed_files, 1);
    assert_eq!(second.new_files, 1);
    assert_eq!(second.deleted_files, 1);
    assert_eq!(second.unchanged_files, 1);
    assert_eq!(
        second.project.generation,
        Generation::new(first.project.generation.value() + 1)
    );
    assert!(
        nodes_for(&index, &first.project.id, "src/change.rs")
            .iter()
            .any(|node| node.name == "new_name")
    );
    assert_eq!(
        index
            .repository()
            .get_file(&goldeneye_domain::FileId::new(
                first.project.id.clone(),
                ProjectRelativePath::new("pkg/delete.py").expect("path"),
            ))
            .expect("deleted file"),
        None
    );
    assert_eq!(
        nodes_for(&index, &first.project.id, "web/keep.js")
            .into_iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        stable_ids
    );
}

#[test]
fn malformed_targeted_refresh_preserves_prior_committed_graph() {
    let temp = TempDir::new().expect("temp repo");
    write(temp.path(), "src/lib.rs", "fn healthy() {}\n");
    let mut index = service(IndexOptions::default());
    let first = index.index_repository(temp.path()).expect("initial");
    let original = nodes_for(&index, &first.project.id, "src/lib.rs");

    write(temp.path(), "src/lib.rs", "fn broken(\n");
    let rejected = index
        .refresh_file(
            &first.project.id,
            &ProjectRelativePath::new("src/lib.rs").expect("path"),
        )
        .expect("refresh result");
    assert_eq!(rejected.status, FileRefreshStatus::RejectedSyntax);
    assert!(!rejected.diagnostics.is_empty());
    assert_eq!(rejected.generation, first.project.generation);
    assert_eq!(nodes_for(&index, &first.project.id, "src/lib.rs"), original);

    write(temp.path(), "src/lib.rs", "fn repaired() {}\n");
    let updated = index
        .refresh_file(
            &first.project.id,
            &ProjectRelativePath::new("src/lib.rs").expect("path"),
        )
        .expect("valid refresh");
    assert_eq!(updated.status, FileRefreshStatus::Updated);
    assert_eq!(
        updated.generation,
        Generation::new(first.project.generation.value() + 1)
    );
    assert!(
        nodes_for(&index, &first.project.id, "src/lib.rs")
            .iter()
            .any(|node| node.name == "repaired")
    );
}

#[test]
fn malformed_incremental_change_rejects_commit_and_preserves_generation() {
    let temp = TempDir::new().expect("temp repo");
    write(temp.path(), "src/lib.rs", "fn healthy() {}\n");
    let mut index = service(IndexOptions::default());
    let first = index.index_repository(temp.path()).expect("initial");
    let path = ProjectRelativePath::new("src/lib.rs").expect("path");
    let file_id = goldeneye_domain::FileId::new(first.project.id.clone(), path);
    let original_file = index
        .repository()
        .get_file(&file_id)
        .expect("file lookup")
        .expect("file");
    let original_nodes = index.repository().nodes_for_file(&file_id).expect("nodes");

    write(temp.path(), "src/lib.rs", "fn broken(\n");
    let rejected = index
        .index_repository(temp.path())
        .expect("diagnostic result");

    assert_eq!(rejected.status, IndexStatus::RejectedSyntax);
    assert_eq!(rejected.project.generation, first.project.generation);
    assert_eq!(rejected.changed_files, 1);
    assert!(!rejected.diagnostics.is_empty());
    assert_eq!(
        index.repository().get_file(&file_id).expect("file"),
        Some(original_file)
    );
    assert_eq!(
        index.repository().nodes_for_file(&file_id).expect("nodes"),
        original_nodes
    );
}

#[test]
fn duplicate_short_names_never_create_false_cross_file_calls() {
    let temp = TempDir::new().expect("temp repo");
    write(temp.path(), "a.py", "def helper():\n    pass\n");
    write(temp.path(), "b.py", "def helper():\n    pass\n");
    write(temp.path(), "caller.py", "def caller():\n    helper()\n");
    let mut index = service(IndexOptions::default());
    let result = index.index_repository(temp.path()).expect("index");

    let caller_nodes = nodes_for(&index, &result.project.id, "caller.py");
    let call_edges = caller_nodes
        .iter()
        .flat_map(|node| {
            index
                .repository()
                .edges_from(&result.project.id, &node.id)
                .expect("edges")
        })
        .filter(|edge| edge.kind == EdgeKind::new("CALLS").expect("edge kind"))
        .collect::<Vec<_>>();
    assert!(call_edges.is_empty(), "unexpected calls: {call_edges:?}");

    write(
        temp.path(),
        "caller.py",
        "def helper():\n    pass\ndef caller():\n    helper()\n",
    );
    let refreshed = index
        .refresh_file(
            &result.project.id,
            &ProjectRelativePath::new("caller.py").expect("path"),
        )
        .expect("refresh");
    assert_eq!(refreshed.status, FileRefreshStatus::Updated);
    let caller_nodes = nodes_for(&index, &result.project.id, "caller.py");
    let local_ids = caller_nodes
        .iter()
        .map(|node| node.id.clone())
        .collect::<BTreeSet<NodeId>>();
    let call_edges = caller_nodes
        .iter()
        .flat_map(|node| {
            index
                .repository()
                .edges_from(&result.project.id, &node.id)
                .expect("edges")
        })
        .filter(|edge| edge.kind.as_str() == "CALLS")
        .collect::<Vec<_>>();
    assert_eq!(call_edges.len(), 1);
    assert!(local_ids.contains(&call_edges[0].source));
    assert!(local_ids.contains(&call_edges[0].target));
}

#[test]
fn unimported_builtins_do_not_link_to_project_lookalikes() {
    let temp = TempDir::new().expect("temp repo");
    write(
        temp.path(),
        "lookalike.py",
        "def print(value):\n    return value\n",
    );
    write(
        temp.path(),
        "caller.py",
        "def caller():\n    print('external')\n",
    );
    let mut index = service(IndexOptions::default());
    let result = index.index_repository(temp.path()).expect("index");

    let calls = nodes_for(&index, &result.project.id, "caller.py")
        .into_iter()
        .flat_map(|node| {
            index
                .repository()
                .edges_from(&result.project.id, &node.id)
                .expect("caller edges")
        })
        .filter(|edge| edge.kind.as_str() == "CALLS")
        .collect::<Vec<_>>();
    assert!(
        calls.is_empty(),
        "builtin linked to project lookalike: {calls:#?}"
    );
}

#[test]
fn targeted_refresh_recomputes_cross_file_calls() {
    let temp = TempDir::new().expect("temp repo");
    write(temp.path(), "target.py", "def helper():\n    return 1\n");
    write(
        temp.path(),
        "caller.py",
        "from target import helper\ndef caller():\n    return helper()\n",
    );
    let mut index = service(IndexOptions::default());
    let result = index.index_repository(temp.path()).expect("index");
    let target_path = ProjectRelativePath::new("target.py").expect("target path");

    let call_count = |index: &IndexService<CoreGrammarProvider, Store>| {
        nodes_for(index, &result.project.id, "caller.py")
            .into_iter()
            .flat_map(|node| {
                index
                    .repository()
                    .edges_from(&result.project.id, &node.id)
                    .expect("caller edges")
            })
            .filter(|edge| edge.kind.as_str() == "CALLS")
            .count()
    };
    assert_eq!(call_count(&index), 1);

    write(temp.path(), "target.py", "def helper():\n    return 2\n");
    let refreshed = index
        .refresh_file(&result.project.id, &target_path)
        .expect("refresh stable target");
    assert_eq!(refreshed.status, FileRefreshStatus::Updated);
    assert_eq!(call_count(&index), 1);

    write(temp.path(), "target.py", "def renamed():\n    return 3\n");
    index
        .refresh_file(&result.project.id, &target_path)
        .expect("refresh renamed target");
    assert_eq!(call_count(&index), 0);
}

#[test]
fn bounds_and_cancellation_abort_before_registration() {
    let temp = TempDir::new().expect("temp repo");
    write(temp.path(), "one.rs", "fn one() {}\n");
    write(temp.path(), "two.py", "def two():\n    pass\n");

    let bounded_options = IndexOptions {
        max_files: Some(1),
        ..IndexOptions::default()
    };
    let mut bounded = service(bounded_options);
    assert!(matches!(
        bounded.index_repository(temp.path()),
        Err(IndexError::FileLimitExceeded {
            limit: 1,
            actual: 2
        })
    ));
    assert!(
        bounded
            .repository()
            .list_projects()
            .expect("projects")
            .is_empty()
    );

    let token = CancellationToken::new();
    token.cancel();
    let cancelled_options = IndexOptions {
        cancellation: token,
        ..IndexOptions::default()
    };
    let mut cancelled = service(cancelled_options);
    assert!(matches!(
        cancelled.index_repository(temp.path()),
        Err(IndexError::Cancelled)
    ));
    assert!(
        cancelled
            .repository()
            .list_projects()
            .expect("projects")
            .is_empty()
    );
}

#[test]
fn normalized_core_fixture_matches_pinned_upstream_fast_graph() {
    let temp = TempDir::new().expect("temp repo");
    write(
        temp.path(),
        "rust.rs",
        "struct Point;\nfn rust_leaf() {}\nfn rust_caller() { rust_leaf(); }\n",
    );
    write(
        temp.path(),
        "python.py",
        "class Dog:\n    pass\ndef py_leaf():\n    pass\ndef py_caller():\n    py_leaf()\n",
    );
    write(
        temp.path(),
        "javascript.js",
        "class Counter {}\nfunction js_leaf() {}\nfunction js_caller() { js_leaf(); }\n",
    );
    write(
        temp.path(),
        "typescript.ts",
        "interface Runner {}\nclass Service {}\nfunction ts_leaf(): void {}\nfunction ts_caller(): void { ts_leaf(); }\n",
    );
    write(
        temp.path(),
        "go.go",
        "package main\ntype Server struct {}\nfunc goLeaf() {}\nfunc goCaller() { goLeaf() }\n",
    );
    let mut index = service(IndexOptions::default());
    let result = index.index_repository(temp.path()).expect("index");
    let prefix = result.project.id.as_str();

    assert_eq!(
        result.counts,
        goldeneye_store::GraphCounts {
            files: 5,
            nodes: 27,
            edges: 32,
        }
    );
    let branch = index
        .repository()
        .node_by_qualified_name(
            &result.project.id,
            &goldeneye_domain::QualifiedName::new(format!("{prefix}.__branch__.working-tree"))
                .expect("branch QN"),
        )
        .expect("branch lookup")
        .expect("branch node");
    assert_eq!(branch.label.as_str(), "Branch");
    for qualified_name in [
        format!("{prefix}.rust.__file__"),
        format!("{prefix}.rust.rust_leaf"),
        format!("{prefix}.python.py_leaf"),
        format!("{prefix}.javascript.js_leaf"),
        format!("{prefix}.typescript.ts_leaf"),
        format!("{prefix}.goLeaf"),
    ] {
        assert!(
            index
                .repository()
                .node_by_qualified_name(
                    &result.project.id,
                    &goldeneye_domain::QualifiedName::new(qualified_name.clone())
                        .expect("fixture QN"),
                )
                .expect("node lookup")
                .is_some(),
            "missing {qualified_name}"
        );
    }
    for caller in [
        format!("{prefix}.rust.rust_caller"),
        format!("{prefix}.python.py_caller"),
        format!("{prefix}.javascript.js_caller"),
        format!("{prefix}.typescript.ts_caller"),
        format!("{prefix}.goCaller"),
    ] {
        let caller = index
            .repository()
            .node_by_qualified_name(
                &result.project.id,
                &goldeneye_domain::QualifiedName::new(caller).expect("caller QN"),
            )
            .expect("caller lookup")
            .expect("caller");
        let calls = index
            .repository()
            .edges_from(&result.project.id, &caller.id)
            .expect("caller edges")
            .into_iter()
            .filter(|edge| edge.kind.as_str() == "CALLS")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].properties["line"].as_u64().is_some());
    }
}

#[test]
fn go_files_in_one_directory_share_a_directory_qualified_module() {
    let temp = TempDir::new().expect("temp repo");
    write(temp.path(), "cmd/main.go", "package main\nfunc main() {}\n");
    write(
        temp.path(),
        "cmd/main_test.go",
        "package main\nfunc TestMain() {}\n",
    );
    let mut index = service(IndexOptions::default());

    let result = index
        .index_repository(temp.path())
        .expect("index Go package");
    let prefix = result.project.id.as_str();
    let mut modules = index
        .repository()
        .list_nodes(&result.project.id)
        .expect("project nodes")
        .into_iter()
        .filter(|node| node.label.as_str() == "Module")
        .map(|node| node.qualified_name.as_str().to_owned())
        .collect::<Vec<_>>();
    modules.sort();

    assert_eq!(modules, [format!("{prefix}.cmd")]);
    for qualified_name in [
        format!("{prefix}.cmd.main"),
        format!("{prefix}.cmd.TestMain"),
    ] {
        assert!(
            index
                .repository()
                .node_by_qualified_name(
                    &result.project.id,
                    &goldeneye_domain::QualifiedName::new(qualified_name.clone())
                        .expect("function QN"),
                )
                .expect("function lookup")
                .is_some(),
            "missing {qualified_name}"
        );
    }

    remove(temp.path(), "cmd/main.go");
    let refreshed = index
        .index_repository(temp.path())
        .expect("reindex remaining Go package file");
    let module = index
        .repository()
        .node_by_qualified_name(
            &refreshed.project.id,
            &goldeneye_domain::QualifiedName::new(format!("{prefix}.cmd")).expect("module QN"),
        )
        .expect("module lookup")
        .expect("remaining module");
    let test_main = index
        .repository()
        .node_by_qualified_name(
            &refreshed.project.id,
            &goldeneye_domain::QualifiedName::new(format!("{prefix}.cmd.TestMain"))
                .expect("function QN"),
        )
        .expect("function lookup")
        .expect("remaining function");
    assert!(
        index
            .repository()
            .edges_from(&refreshed.project.id, &module.id)
            .expect("module edges")
            .iter()
            .any(|edge| edge.target == test_main.id)
    );
}
