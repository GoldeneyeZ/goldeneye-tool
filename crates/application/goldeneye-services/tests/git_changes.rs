use std::fs;
use std::process::Command;
use std::sync::Arc;

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_domain::{
    ContentHash, EdgeKind, FileId, FileRecord, Generation, GraphEdge, GraphNode, NodeId, NodeLabel,
    ProjectId, ProjectRecord, ProjectRelativePath, QualifiedName,
};
use goldeneye_git::GitCommandRepository;
use goldeneye_services::{
    CancellationToken, DetectChangesRequest, ServiceConfig, ServiceDependencies, Services,
};
use goldeneye_store::Store;

fn service_dependencies() -> ServiceDependencies {
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
    )
}

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Goldeneye")
        .env("GIT_AUTHOR_EMAIL", "goldeneye@example.test")
        .env("GIT_COMMITTER_NAME", "Goldeneye")
        .env("GIT_COMMITTER_EMAIL", "goldeneye@example.test")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

fn rel(path: &str) -> ProjectRelativePath {
    ProjectRelativePath::new(path).expect("relative path")
}

fn file(project: &ProjectId, path: &str) -> FileRecord {
    FileRecord::new(
        FileId::new(project.clone(), rel(path)),
        ContentHash::of(path.as_bytes()),
        Generation::new(0),
        0,
        1,
    )
}

fn node(project: &ProjectId, id: &str, label: &str, name: &str, path: &str) -> GraphNode {
    GraphNode::new(
        project.clone(),
        NodeId::new(id).expect("node id"),
        NodeLabel::new(label).expect("label"),
        name,
        QualifiedName::new(format!("demo.{path}.{name}")).expect("qualified name"),
        Some(rel(path)),
        None,
        Generation::new(0),
    )
    .expect("node")
}

#[test]
fn history_enrichment_and_change_blast_radius_are_end_to_end() {
    let temp = tempfile::tempdir().expect("temp");
    let repo = temp.path().join("repo");
    fs::create_dir(&repo).expect("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    for revision in 0..3 {
        fs::write(
            repo.join("a.rs"),
            format!("fn a() {{ /* {revision} */ }}\n"),
        )
        .expect("a");
        fs::write(
            repo.join("b.rs"),
            format!("fn b() {{ /* {revision} */ }}\n"),
        )
        .expect("b");
        git(&repo, &["add", "a.rs", "b.rs"]);
        git(
            &repo,
            &["commit", "-q", "-m", &format!("revision {revision}")],
        );
    }

    let database = temp.path().join("graph.sqlite3");
    let project_id = ProjectId::new("demo").expect("project id");
    let project = ProjectRecord::new(
        project_id.clone(),
        repo.canonicalize()
            .expect("canonical repo")
            .to_string_lossy(),
    )
    .expect("project");
    let mut store = Store::open(&database).expect("store");
    store
        .replace_project_graph(
            &project,
            vec![file(&project_id, "a.rs"), file(&project_id, "b.rs")],
            vec![
                node(&project_id, "file:a", "File", "a.rs", "a.rs"),
                node(&project_id, "file:b", "File", "b.rs", "b.rs"),
                node(&project_id, "fn:a", "Function", "a", "a.rs"),
                node(&project_id, "fn:b", "Function", "b", "b.rs"),
            ],
            vec![GraphEdge::new(
                project_id.clone(),
                NodeId::new("fn:b").expect("source"),
                NodeId::new("fn:a").expect("target"),
                EdgeKind::new("CALLS").expect("kind"),
                Generation::new(0),
            )],
        )
        .expect("graph");
    drop(store);

    let services = Services::new(ServiceConfig::new(&database, &repo), service_dependencies());
    let token = CancellationToken::new();
    let history = services
        .refresh_git_history(&project_id, &token)
        .expect("history");
    assert_eq!(history.couplings, 1);
    assert_eq!(history.enriched_edges, 1);
    let context = services.git_context(&project_id, &token).expect("context");
    assert!(context.is_git);
    assert_eq!(context.branch, "main");

    fs::write(repo.join("a.rs"), "fn changed() {}\n").expect("dirty");
    fs::write(repo.join("untracked.rs"), "fn untracked() {}\n").expect("untracked");
    let result = services
        .detect_changes(&DetectChangesRequest::new(project_id.clone()), &token)
        .expect("changes");
    assert_eq!(result.changed_files, vec!["a.rs", "untracked.rs"]);
    let names = result
        .impacted_symbols
        .iter()
        .map(|symbol| symbol.name.as_str())
        .collect::<Vec<_>>();
    assert!(names.contains(&"a"));
    assert!(names.contains(&"b"));

    let mut files_only = DetectChangesRequest::new(project_id);
    files_only.scope = Some("files".to_owned());
    assert!(
        services
            .detect_changes(&files_only, &token)
            .expect("files only")
            .impacted_symbols
            .is_empty()
    );
}
