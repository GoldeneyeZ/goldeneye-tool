#![allow(clippy::float_cmp)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_domain::{
    EdgeKind, Generation, GraphEdge, GraphNode, NodeId, NodeLabel, ProjectRecord, QualifiedName,
};
use goldeneye_git::GitCommandRepository;
use goldeneye_ports::{
    ArtifactPersistence, CrossLinkRepository, DetectChangesOptions, DetectedChanges,
    EditRepository, GitContext, GitHistory, GitPortError, GitRepository, IndexRepository,
    LanguageClassifier, PortError, QueryRepository, RepositoryDiscovery,
    RepositoryDiscoveryOptions, RepositoryDiscoveryReport, RepositoryFactory,
};
use goldeneye_services::{
    ArchitectureRequest, CancellationToken, CodeSnippetRequest, CreateFileRequest,
    DetectChangesRequest, GraphSchemaRequest, IndexRepositoryRequest, IndexRepositoryResult,
    IndexStatusRequest, InspectSyntaxRequest, LanguageId, NodeContentRequest, OperationHooks,
    PageRequest, ProjectId, ProjectRelativePath, QueryGraphRequest, SearchCodeRequest,
    SearchCodeResult, SearchGraphRequest, ServiceConfig, ServiceDependencies, ServiceErrorCode,
    Services, TraceDirection, TracePathRequest,
};
use goldeneye_store::{SqliteRepositoryFactory, Store};
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
use tempfile::TempDir;

fn service_dependencies() -> ServiceDependencies {
    let discovery = Arc::new(FileSystemDiscovery);
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
        discovery,
        Arc::new(SqliteRepositoryFactory),
        Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
        Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
    )
}

#[derive(Default)]
struct RecordingSourceDiscovery {
    discovery_calls: AtomicUsize,
    classified_paths: Mutex<Vec<PathBuf>>,
}

struct FailingRepositoryFactory;

struct FailSecondCrosslinkFactory;

struct FailSecondCrosslinkRepository {
    store: Store,
    replacements: usize,
}

impl RepositoryFactory for FailingRepositoryFactory {
    fn initialize(&self, _path: &Path) -> Result<(), PortError> {
        Err(repository_failure())
    }

    fn open_query(&self, _path: &Path) -> Result<Box<dyn QueryRepository>, PortError> {
        Err(repository_failure())
    }

    fn open_index(&self, _path: &Path) -> Result<Box<dyn IndexRepository>, PortError> {
        Err(repository_failure())
    }

    fn open_edit(&self, _path: &Path) -> Result<Box<dyn EditRepository>, PortError> {
        Err(repository_failure())
    }

    fn open_crosslink(&self, _path: &Path) -> Result<Box<dyn CrossLinkRepository>, PortError> {
        Err(repository_failure())
    }
}

impl RepositoryFactory for FailSecondCrosslinkFactory {
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
        Ok(Box::new(FailSecondCrosslinkRepository {
            store: Store::open(path).map_err(PortError::new)?,
            replacements: 0,
        }))
    }
}

impl CrossLinkRepository for FailSecondCrosslinkRepository {
    fn list_projects(&self) -> Result<Vec<ProjectRecord>, PortError> {
        self.store.list_projects().map_err(PortError::new)
    }

    fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, PortError> {
        self.store.list_nodes(project).map_err(PortError::new)
    }

    fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, PortError> {
        self.store.list_edges(project).map_err(PortError::new)
    }

    fn replace_cross_project_edges(
        &mut self,
        project: &ProjectId,
        edges: &[GraphEdge],
    ) -> Result<usize, PortError> {
        self.replacements += 1;
        if self.replacements == 2 {
            return Err(PortError::new(std::io::Error::other(
                "forced second crosslink replacement failure",
            )));
        }
        self.store
            .replace_cross_project_edges(project, edges)
            .map_err(PortError::new)
    }
}

fn repository_failure() -> PortError {
    PortError::new(std::io::Error::other("repository factory failed"))
}

impl RepositoryDiscovery for RecordingSourceDiscovery {
    fn discover(
        &self,
        root: &Path,
        options: &RepositoryDiscoveryOptions,
    ) -> Result<RepositoryDiscoveryReport, PortError> {
        self.discovery_calls.fetch_add(1, Ordering::Relaxed);
        RepositoryDiscovery::discover(&FileSystemDiscovery, root, options)
    }
}

impl LanguageClassifier for RecordingSourceDiscovery {
    fn classify(&self, path: &Path) -> Option<LanguageId> {
        self.classified_paths
            .lock()
            .expect("classified paths")
            .push(path.to_path_buf());
        LanguageClassifier::classify(&FileSystemDiscovery, path)
    }
}

#[derive(Debug, Default)]
struct ArtifactCalls {
    imports: Vec<(PathBuf, PathBuf, bool)>,
    exports: Vec<(PathBuf, PathBuf, String)>,
}

struct RecordingArtifact {
    exists: bool,
    fail_import: bool,
    fail_export: bool,
    calls: Arc<Mutex<ArtifactCalls>>,
}

struct FailingGit {
    invalid_reference: bool,
    cancel_history: bool,
}

impl GitRepository for FailingGit {
    fn validate_reference(&self, _reference: &str) -> Result<(), GitPortError> {
        if self.invalid_reference {
            return Err(GitPortError::InvalidReference);
        }
        Ok(())
    }

    fn resolve_context(
        &self,
        _root: &Path,
        _cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<GitContext, GitPortError> {
        Err(GitPortError::Adapter(PortError::new(
            std::io::Error::other("context failed"),
        )))
    }

    fn collect_history(
        &self,
        _root: &Path,
        _cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<GitHistory, GitPortError> {
        if self.cancel_history {
            return Err(GitPortError::Cancelled);
        }
        Err(GitPortError::Adapter(PortError::new(
            std::io::Error::other("history failed"),
        )))
    }

    fn detect_changes(
        &self,
        _root: &Path,
        _options: &DetectChangesOptions,
        _cancellation: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<DetectedChanges, GitPortError> {
        Err(GitPortError::Adapter(PortError::new(
            std::io::Error::other("changes failed"),
        )))
    }
}

impl ArtifactPersistence for RecordingArtifact {
    fn exists(&self, _repository: &Path) -> bool {
        self.exists
    }

    fn import(&self, repository: &Path, database: &Path) -> Result<(), PortError> {
        self.calls.lock().expect("artifact calls").imports.push((
            repository.to_path_buf(),
            database.to_path_buf(),
            database.is_file(),
        ));
        if self.fail_import {
            return Err(PortError::new(std::io::Error::other("import failed")));
        }
        Ok(())
    }

    fn export(&self, database: &Path, repository: &Path, project: &str) -> Result<(), PortError> {
        self.calls.lock().expect("artifact calls").exports.push((
            database.to_path_buf(),
            repository.to_path_buf(),
            project.to_owned(),
        ));
        if self.fail_export {
            return Err(PortError::new(std::io::Error::other("export failed")));
        }
        Ok(())
    }
}

fn recording_dependencies(
    exists: bool,
    fail_import: bool,
    fail_export: bool,
) -> (ServiceDependencies, Arc<Mutex<ArtifactCalls>>) {
    let calls = Arc::new(Mutex::new(ArtifactCalls::default()));
    let artifact = RecordingArtifact {
        exists,
        fail_import,
        fail_export,
        calls: Arc::clone(&calls),
    };
    (
        ServiceDependencies::new(
            Arc::new(artifact),
            Arc::new(GitCommandRepository),
            Arc::new(FileSystemDiscovery),
            Arc::new(SqliteRepositoryFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
        ),
        calls,
    )
}

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
    let services = Services::new(config.clone(), service_dependencies());

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
    let reopened = Services::new(config, service_dependencies());
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
fn injected_source_discovery_drives_indexing_and_language_classification() {
    let temp = TempDir::new().expect("temp directory");
    let allowed = temp.path().join("allowed");
    let repo = allowed.join("fixture");
    write_fixture(&repo);
    let source = Arc::new(RecordingSourceDiscovery::default());
    let services = Services::new(
        ServiceConfig::new(temp.path().join("graph.db"), &allowed).with_allowed_root(&allowed),
        ServiceDependencies::new(
            Arc::new(FileArtifactPersistence),
            Arc::new(GitCommandRepository),
            source.clone(),
            Arc::new(SqliteRepositoryFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
        ),
    );

    let indexed = services
        .index_repository(&IndexRepositoryRequest::new(&repo))
        .expect("index fixture");
    assert_eq!(source.discovery_calls.load(Ordering::Relaxed), 1);

    services
        .inspect_syntax(&InspectSyntaxRequest::new(
            ProjectId::new(indexed.project).expect("project ID"),
            ProjectRelativePath::new("src/lib.rs").expect("relative path"),
        ))
        .expect("inspect source");
    assert_eq!(
        source
            .classified_paths
            .lock()
            .expect("classified paths")
            .as_slice(),
        [PathBuf::from("src/lib.rs")]
    );
}

#[test]
fn repository_factory_failures_keep_repository_storage_classification_and_message() {
    let temp = TempDir::new().expect("temp directory");
    let services = Services::new(
        ServiceConfig::new(temp.path().join("missing/graph.db"), temp.path())
            .with_allowed_root(temp.path()),
        ServiceDependencies::new(
            Arc::new(FileArtifactPersistence),
            Arc::new(GitCommandRepository),
            Arc::new(FileSystemDiscovery),
            Arc::new(FailingRepositoryFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
        ),
    );

    let error = services
        .list_projects()
        .expect_err("repository initialization must fail");

    assert_eq!(error.code(), ServiceErrorCode::Storage);
    assert!(matches!(
        &error,
        goldeneye_services::ServiceError::Repository(_)
    ));
    assert_eq!(error.to_string(), "repository factory failed");
}

#[test]
fn failed_crosslink_rebuild_invalidates_graphs_changed_before_the_failure() {
    let temp = TempDir::new().expect("temp directory");
    let database = temp.path().join("graph.db");
    let first_id = ProjectId::new("first").expect("first project ID");
    let second_id = ProjectId::new("second").expect("second project ID");
    let first = ProjectRecord::new(
        first_id.clone(),
        temp.path().join("first").to_string_lossy(),
    )
    .expect("first project");
    let second = ProjectRecord::new(
        second_id.clone(),
        temp.path().join("second").to_string_lossy(),
    )
    .expect("second project");
    let pending = Generation::new(0);
    let source = GraphNode::new(
        first_id.clone(),
        NodeId::new("source").expect("source node ID"),
        NodeLabel::new("Function").expect("source label"),
        "source",
        QualifiedName::new("first.source").expect("source qualified name"),
        None,
        None,
        pending,
    )
    .expect("source node");
    let target = GraphNode::new(
        first_id.clone(),
        NodeId::new("target").expect("target node ID"),
        NodeLabel::new("Function").expect("target label"),
        "target",
        QualifiedName::new("first.target").expect("target qualified name"),
        None,
        None,
        pending,
    )
    .expect("target node");
    let stale_cross_edge = GraphEdge::new(
        first_id.clone(),
        source.id.clone(),
        target.id.clone(),
        EdgeKind::new("CROSS_HTTP_CALLS").expect("cross edge kind"),
        pending,
    );
    let mut store = Store::open(&database).expect("open store");
    store
        .replace_project_graph(
            &first,
            Vec::new(),
            vec![source, target],
            vec![stale_cross_edge],
        )
        .expect("seed first graph");
    store
        .replace_project_graph(&second, Vec::new(), Vec::new(), Vec::new())
        .expect("seed second graph");
    drop(store);

    let services = Services::new(
        ServiceConfig::new(&database, temp.path()),
        ServiceDependencies::new(
            Arc::new(FileArtifactPersistence),
            Arc::new(GitCommandRepository),
            Arc::new(FileSystemDiscovery),
            Arc::new(FailSecondCrosslinkFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
        ),
    );
    let warmed = services
        .get_architecture(&ArchitectureRequest::new(first_id.clone()))
        .expect("warm first architecture");
    assert_eq!(warmed.total_edges, 1);

    let error = services
        .rebuild_cross_repo_intelligence()
        .expect_err("second replacement must fail");
    assert!(
        error
            .to_string()
            .contains("forced second crosslink replacement failure"),
        "{error}"
    );
    let reloaded = services
        .get_architecture(&ArchitectureRequest::new(first_id))
        .expect("reload first architecture");
    assert_eq!(reloaded.total_edges, 0);
}

#[test]
fn project_name_override_is_sanitized_and_persisted() {
    let temp = TempDir::new().expect("temp directory");
    let allowed = temp.path().join("allowed");
    let repo = allowed.join("fixture");
    write_fixture(&repo);
    let services = Services::new(
        ServiceConfig::new(temp.path().join("graph.db"), &allowed).with_allowed_root(&allowed),
        service_dependencies(),
    );

    let indexed = services
        .index_repository(&IndexRepositoryRequest::new(&repo).with_name("Team API"))
        .expect("index named project");

    assert_eq!(indexed.project, "Team-API");
    assert_eq!(
        services.list_projects().expect("projects")[0].project,
        "Team-API"
    );
    assert!(
        services
            .index_status(&IndexStatusRequest::new(
                ProjectId::new("Team-API").expect("project ID"),
            ))
            .expect("named project status")
            .nodes
            > 0
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
        service_dependencies(),
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
        service_dependencies(),
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

#[test]
fn artifact_import_is_best_effort_before_store_open_and_existing_artifact_is_refreshed() {
    let temp = TempDir::new().expect("temp dir");
    let repo = temp.path().join("fixture");
    let database = temp.path().join("state/graph.db");
    write_fixture(&repo);
    let (dependencies, calls) = recording_dependencies(true, true, false);
    let services = Services::new(
        ServiceConfig::new(&database, temp.path()).with_allowed_root(temp.path()),
        dependencies,
    );

    let indexed = services
        .index_repository(&IndexRepositoryRequest::new(&repo))
        .expect("best-effort import must not block indexing");

    let resolved_repo = fs::canonicalize(&repo).expect("canonical repository");
    let calls = calls.lock().expect("artifact calls");
    assert_eq!(calls.imports.len(), 1);
    assert_eq!(
        calls.imports[0],
        (resolved_repo.clone(), database.clone(), false)
    );
    assert_eq!(calls.exports.len(), 1);
    assert_eq!(calls.exports[0].0, database);
    assert_eq!(calls.exports[0].1, resolved_repo);
    assert_eq!(calls.exports[0].2, indexed.project);
}

#[test]
fn artifact_export_is_opt_in_and_failure_maps_to_storage() {
    let temp = TempDir::new().expect("temp dir");
    let repo = temp.path().join("fixture");
    let database = temp.path().join("graph.db");
    write_fixture(&repo);
    let (dependencies, calls) = recording_dependencies(false, false, true);
    let services = Services::new(
        ServiceConfig::new(&database, temp.path()).with_allowed_root(temp.path()),
        dependencies,
    );

    services
        .index_repository(&IndexRepositoryRequest::new(&repo))
        .expect("index without persistence");
    assert!(calls.lock().expect("artifact calls").exports.is_empty());

    let error = services
        .index_repository(&IndexRepositoryRequest::new(&repo).with_persistence(true))
        .expect_err("requested export failure must propagate");
    assert_eq!(error.code(), ServiceErrorCode::Storage);
    assert_eq!(calls.lock().expect("artifact calls").exports.len(), 1);
}

#[test]
fn automatic_git_history_adapter_failures_are_downgraded_to_warnings() {
    let temp = TempDir::new().expect("temp dir");
    let repo = temp.path().join("fixture");
    write_fixture(&repo);
    let dependencies = ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(FailingGit {
            invalid_reference: false,
            cancel_history: false,
        }),
        Arc::new(FileSystemDiscovery),
        Arc::new(SqliteRepositoryFactory),
        Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
        Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
    );
    let services = Services::new(
        ServiceConfig::new(temp.path().join("graph.db"), temp.path())
            .with_allowed_root(temp.path()),
        dependencies,
    );

    let indexed = services
        .index_repository(&IndexRepositoryRequest::new(&repo))
        .expect("history failure must not block indexing");
    assert!(
        indexed
            .warnings
            .iter()
            .any(|warning| warning == "git_history: history failed")
    );
    let project = ProjectId::new(indexed.project).expect("project ID");
    let error = services
        .git_context(&project, &CancellationToken::new())
        .expect_err("adapter context error");
    assert_eq!(error.code(), ServiceErrorCode::Index);
    assert_eq!(error.to_string(), "context failed");
}

#[test]
fn automatic_git_history_cancellation_is_hard_and_invalid_refs_precede_project_lookup() {
    let temp = TempDir::new().expect("temp dir");
    let repo = temp.path().join("fixture");
    write_fixture(&repo);
    let cancelling = Services::new(
        ServiceConfig::new(temp.path().join("cancelled.db"), temp.path())
            .with_allowed_root(temp.path()),
        ServiceDependencies::new(
            Arc::new(FileArtifactPersistence),
            Arc::new(FailingGit {
                invalid_reference: false,
                cancel_history: true,
            }),
            Arc::new(FileSystemDiscovery),
            Arc::new(SqliteRepositoryFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
        ),
    );
    let error = cancelling
        .index_repository(&IndexRepositoryRequest::new(&repo))
        .expect_err("history cancellation must stop indexing");
    assert_eq!(error.code(), ServiceErrorCode::Cancelled);
    assert_eq!(error.to_string(), "index operation was cancelled");

    let validating = Services::new(
        ServiceConfig::new(temp.path().join("validation.db"), temp.path())
            .with_allowed_root(temp.path()),
        ServiceDependencies::new(
            Arc::new(FileArtifactPersistence),
            Arc::new(FailingGit {
                invalid_reference: true,
                cancel_history: false,
            }),
            Arc::new(FileSystemDiscovery),
            Arc::new(SqliteRepositoryFactory),
            Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
            Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
        ),
    );
    let missing = ProjectId::new("missing").expect("project ID");
    let mut request = DetectChangesRequest::new(missing);
    request.base_branch = "--unsafe".to_owned();
    let error = validating
        .detect_changes(&request, &CancellationToken::new())
        .expect_err("invalid ref must precede missing project lookup");
    assert_eq!(error.code(), ServiceErrorCode::InvalidInput);
    assert_eq!(error.to_string(), "base_branch contains invalid characters");
}
