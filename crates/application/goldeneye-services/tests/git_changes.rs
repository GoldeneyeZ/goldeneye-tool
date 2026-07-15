use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_domain::{
    ContentHash, EdgeKind, FileId, FileRecord, Generation, GraphEdge, GraphNode, NodeId, NodeLabel,
    ProjectId, ProjectRecord, ProjectRelativePath, QualifiedName,
};
use goldeneye_git::GitCommandRepository;
use goldeneye_ports::{
    AdrTraceRepository, CrossLinkRepository, EditRepository, GitHistoryRepository, IndexRepository,
    PortError, ProjectAdministrationRepository, QueryRepository, RepositoryFactory,
    SemanticIndexRepository,
};
use goldeneye_services::{
    ArchitectureRequest, CancellationToken, DetectChangesRequest, ServiceConfig,
    ServiceDependencies, ServiceErrorCode, Services,
};
use goldeneye_store::{SqliteRepositoryFactory, Store};
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;

fn service_dependencies() -> ServiceDependencies {
    service_dependencies_with_repositories(Arc::new(SqliteRepositoryFactory))
}

fn service_dependencies_with_repositories(
    repositories: Arc<dyn RepositoryFactory>,
) -> ServiceDependencies {
    let discovery = Arc::new(FileSystemDiscovery);
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
        discovery,
        repositories,
        Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
        Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
    )
}

struct FailingGitHistoryFactory;

impl RepositoryFactory for FailingGitHistoryFactory {
    fn initialize(&self, path: &Path) -> Result<(), PortError> {
        RepositoryFactory::initialize(&SqliteRepositoryFactory, path)
    }

    fn open_query(&self, path: &Path) -> Result<Box<dyn QueryRepository>, PortError> {
        RepositoryFactory::open_query(&SqliteRepositoryFactory, path)
    }

    fn open_index(&self, path: &Path) -> Result<Box<dyn IndexRepository>, PortError> {
        RepositoryFactory::open_index(&SqliteRepositoryFactory, path)
    }

    fn open_edit(&self, path: &Path) -> Result<Box<dyn EditRepository>, PortError> {
        RepositoryFactory::open_edit(&SqliteRepositoryFactory, path)
    }

    fn open_crosslink(&self, path: &Path) -> Result<Box<dyn CrossLinkRepository>, PortError> {
        RepositoryFactory::open_crosslink(&SqliteRepositoryFactory, path)
    }

    fn open_project_administration(
        &self,
        path: &Path,
    ) -> Result<Box<dyn ProjectAdministrationRepository>, PortError> {
        RepositoryFactory::open_project_administration(&SqliteRepositoryFactory, path)
    }

    fn open_adr_traces(&self, path: &Path) -> Result<Box<dyn AdrTraceRepository>, PortError> {
        RepositoryFactory::open_adr_traces(&SqliteRepositoryFactory, path)
    }

    fn open_git_history(&self, _path: &Path) -> Result<Box<dyn GitHistoryRepository>, PortError> {
        Err(PortError::new(std::io::Error::other(
            "Git-history repository unavailable",
        )))
    }

    fn open_semantic_index(
        &self,
        path: &Path,
    ) -> Result<Box<dyn SemanticIndexRepository>, PortError> {
        RepositoryFactory::open_semantic_index(&SqliteRepositoryFactory, path)
    }
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
    let before_history = services
        .get_architecture(&ArchitectureRequest::new(project_id.clone()))
        .expect("architecture before history");
    assert_eq!(before_history.total_edges, 1);
    let history = services
        .refresh_git_history(&project_id, &token)
        .expect("history");
    assert_eq!(history.couplings, 1);
    assert_eq!(history.enriched_edges, 1);
    let after_history = services
        .get_architecture(&ArchitectureRequest::new(project_id.clone()))
        .expect("architecture after history");
    assert_eq!(after_history.total_edges, 2);
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

    let mut files_only = DetectChangesRequest::new(project_id.clone());
    files_only.scope = Some("files".to_owned());
    assert!(
        services
            .detect_changes(&files_only, &token)
            .expect("files only")
            .impacted_symbols
            .is_empty()
    );

    assert_failed_refresh_preserves_cache(&repo, &database, &project_id, &token);
}

fn assert_failed_refresh_preserves_cache(
    repo: &Path,
    database: &Path,
    project_id: &ProjectId,
    token: &CancellationToken,
) {
    let failing_services = Services::new(
        ServiceConfig::new(database, repo),
        service_dependencies_with_repositories(Arc::new(FailingGitHistoryFactory)),
    );
    assert_eq!(
        failing_services
            .get_architecture(&ArchitectureRequest::new(project_id.clone()))
            .expect("warm architecture before failed refresh")
            .total_edges,
        2
    );
    let error = failing_services
        .refresh_git_history(project_id, token)
        .expect_err("Git-history repository failure");
    assert_eq!(error.code(), ServiceErrorCode::Storage);
    assert!(
        error
            .to_string()
            .contains("Git-history repository unavailable"),
        "{error}"
    );
    Store::open(database)
        .expect("store after failed refresh")
        .replace_git_history(project_id, &[], &[])
        .expect("remove Git enrichment outside services");
    assert_eq!(
        failing_services
            .get_architecture(&ArchitectureRequest::new(project_id.clone()))
            .expect("failed refresh preserves warm architecture cache")
            .total_edges,
        2
    );
}
