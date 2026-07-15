use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_domain::{
    FileContext, Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath,
};
use goldeneye_edit::{
    DurableCreateRequest, DurableEditRequest, DurableEditService, EditOperation, EditOptions,
    FaultInjector, FaultPoint, ParsePolicy,
};
use goldeneye_index::{IndexOptions, IndexService};
use goldeneye_store::Store;
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine, all_named_locators};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    allowed_root: PathBuf,
    root: PathBuf,
    database: PathBuf,
    project: ProjectId,
}

impl Fixture {
    fn new(source: &str) -> Self {
        let temp = TempDir::new().expect("temp fixture");
        let allowed_root = temp.path().to_path_buf();
        let root = allowed_root.join("repo");
        fs::create_dir_all(root.join("src")).expect("create repository");
        fs::write(root.join("src/lib.rs"), source).expect("write source");
        let database = allowed_root.join("graph.sqlite");
        let store = Store::open(&database).expect("open store");
        let mut index = IndexService::new(
            store,
            TreeSitterIndexExtractor::new(CoreGrammarProvider),
            IndexOptions::default(),
            FileSystemDiscovery,
        );
        let indexed = index.index_repository(&root).expect("initial index");
        Self {
            _temp: temp,
            allowed_root,
            root,
            database,
            project: indexed.project.id,
        }
    }

    fn open(&self) -> (DurableEditService, goldeneye_edit::RecoveryReport) {
        let store = Store::open(&self.database).expect("reopen store");
        let index = IndexService::new(
            store,
            TreeSitterIndexExtractor::new(CoreGrammarProvider),
            IndexOptions::default(),
            FileSystemDiscovery,
        );
        let journal = Store::open(&self.database).expect("open edit journal");
        DurableEditService::open(
            index,
            journal,
            SyntaxEngine::new(CoreGrammarProvider),
            vec![self.allowed_root.clone()],
        )
        .expect("open durable edit service")
    }

    fn source_path(&self) -> PathBuf {
        self.root.join("src/lib.rs")
    }
}

fn generation(database: &Path, project: &ProjectId) -> Generation {
    Store::open_read_only(database)
        .expect("open query store")
        .get_project(project)
        .expect("read project")
        .expect("indexed project")
        .generation
}

fn function_locator(_service: &DurableEditService, fixture: &Fixture) -> NodeLocator {
    let source = fs::read(fixture.source_path()).expect("read fixture source");
    let snapshot = SyntaxEngine::new(CoreGrammarProvider)
        .parse(
            LanguageId::new("rust").expect("language"),
            Arc::<[u8]>::from(source),
            generation(&fixture.database, &fixture.project),
        )
        .expect("parse fixture");
    let context = FileContext::new(
        fixture.project.clone(),
        ProjectRelativePath::new("src/lib.rs").expect("relative path"),
    );
    all_named_locators(&snapshot, &context)
        .expect("locators")
        .into_iter()
        .find(|locator| locator.anchor.node_kind == "function_item")
        .expect("function locator")
}

fn edit_request(locator: NodeLocator, operation_id: impl Into<String>) -> DurableEditRequest {
    DurableEditRequest {
        operation_id: operation_id.into(),
        locator,
        operation: EditOperation::Replace("fn after() {}".to_owned()),
        options: EditOptions::default(),
    }
}

#[derive(Debug)]
struct FailOnce {
    point: FaultPoint,
    fired: AtomicBool,
}

impl FailOnce {
    fn new(point: FaultPoint) -> Self {
        Self {
            point,
            fired: AtomicBool::new(false),
        }
    }
}

impl FaultInjector for FailOnce {
    fn check(&self, point: FaultPoint) -> Result<(), String> {
        if point == self.point && !self.fired.swap(true, Ordering::SeqCst) {
            return Err(format!("fault at {point:?}"));
        }
        Ok(())
    }
}

#[test]
fn durable_replace_returns_compact_source_syntax_graph_and_generation_metadata() {
    let fixture = Fixture::new("fn before() {}\n");
    let (mut service, startup) = fixture.open();
    assert!(startup.entries.is_empty());
    let before_generation = generation(&fixture.database, &fixture.project);
    let locator = function_locator(&service, &fixture);

    let result = service
        .edit_node(edit_request(locator, "replace-success"))
        .expect("durable edit");

    assert_eq!(
        fs::read_to_string(fixture.source_path()).unwrap(),
        "fn after() {}\n"
    );
    assert_ne!(result.old_file_hash, Some(result.new_file_hash));
    assert_eq!(result.diff.inserted.as_ref(), b"after");
    assert!(!result.syntax_identities.is_empty());
    assert!(result.graph_changes.added > 0);
    assert!(result.graph_changes.removed > 0);
    assert_eq!(
        result.generation,
        Generation::new(before_generation.value() + 1)
    );
    assert!(result.token_size.approximate_context_tokens > 0);
}

#[test]
fn delete_and_adjacent_insertions_use_the_same_durable_journal_pipeline() {
    let cases = [
        ("delete", EditOperation::Delete, "\n"),
        (
            "insert-before",
            EditOperation::InsertBefore("// inserted\n".to_owned()),
            "// inserted\nfn before() {}\n",
        ),
        (
            "insert-after",
            EditOperation::InsertAfter("\nfn extra() {}".to_owned()),
            "fn before() {}\nfn extra() {}\n",
        ),
    ];

    for (operation_id, operation, expected) in cases {
        let fixture = Fixture::new("fn before() {}\n");
        let (mut service, _) = fixture.open();
        let request = DurableEditRequest {
            operation_id: operation_id.to_owned(),
            locator: function_locator(&service, &fixture),
            operation,
            options: EditOptions::default(),
        };
        service.edit_node(request).expect("durable operation");
        assert_eq!(fs::read_to_string(fixture.source_path()).unwrap(), expected);
        assert!(
            Store::open_read_only(&fixture.database)
                .expect("open query store")
                .list_incomplete_edit_operations()
                .expect("journal")
                .is_empty()
        );
    }
}

#[test]
fn restart_recovery_reconciles_every_incomplete_filesystem_phase() {
    let cases = [
        (FaultPoint::AfterJournal, false),
        (FaultPoint::BeforeWrite, false),
        (FaultPoint::AfterTemp, false),
        (FaultPoint::AfterBackup, false),
        (FaultPoint::AfterRename, true),
        (FaultPoint::DuringReindex, true),
        (FaultPoint::Cleanup, true),
    ];

    for (point, should_commit_new_source) in cases {
        let fixture = Fixture::new("fn before() {}\n");
        let (mut service, _) = fixture.open();
        let locator = function_locator(&service, &fixture);
        service.set_fault_injector(Arc::new(FailOnce::new(point)));
        let error = service
            .edit_node(edit_request(locator, format!("fault-{point:?}")))
            .expect_err("fault must interrupt operation");
        assert!(
            error.to_string().contains("fault"),
            "unexpected error for {point:?}: {error}"
        );
        drop(service);

        let (_service, recovery) = fixture.open();
        assert_eq!(recovery.entries.len(), 1, "recovery report for {point:?}");
        assert!(recovery.entries[0].resolved, "unresolved {point:?}");
        let source = fs::read_to_string(fixture.source_path()).expect("recovered source");
        let expected_name = if should_commit_new_source {
            assert_eq!(source, "fn after() {}\n", "source for {point:?}");
            "after"
        } else {
            assert_eq!(source, "fn before() {}\n", "source for {point:?}");
            "before"
        };
        let file = goldeneye_domain::FileId::new(
            fixture.project.clone(),
            ProjectRelativePath::new("src/lib.rs").unwrap(),
        );
        let nodes = Store::open_read_only(&fixture.database)
            .expect("open query store")
            .nodes_for_file(&file)
            .expect("recovered graph");
        assert!(nodes.iter().any(|node| node.name == expected_name));
    }
}

#[test]
fn stale_source_and_generation_are_rejected_without_writing() {
    let fixture = Fixture::new("fn before() {}\n");
    let (mut service, _) = fixture.open();
    let stale = function_locator(&service, &fixture);
    fs::write(fixture.source_path(), "fn external() {}\n").expect("external edit");

    let error = service
        .edit_node(edit_request(stale, "stale-source"))
        .expect_err("stale locator");
    assert!(error.to_string().contains("stale"));
    assert_eq!(
        fs::read_to_string(fixture.source_path()).unwrap(),
        "fn external() {}\n"
    );

    let generation_fixture = Fixture::new("fn before() {}\n");
    let (mut generation_service, _) = generation_fixture.open();
    let old_locator = function_locator(&generation_service, &generation_fixture);
    generation_service
        .edit_node(edit_request(old_locator.clone(), "advance-generation"))
        .expect("advance generation");
    let error = generation_service
        .edit_node(edit_request(old_locator, "stale-generation"))
        .expect_err("stale generation");
    assert!(
        error.to_string().contains("stale project generation"),
        "unexpected stale-generation error: {error}"
    );
}

#[test]
fn unicode_create_is_no_overwrite_and_indexes_in_one_generation() {
    let fixture = Fixture::new("fn before() {}\n");
    let (mut service, _) = fixture.open();
    let before_generation = generation(&fixture.database, &fixture.project);
    let path = ProjectRelativePath::new("nested/深/naïve.rs").expect("Unicode path");
    let request = DurableCreateRequest {
        operation_id: "unicode-create".to_owned(),
        project_id: fixture.project.clone(),
        relative_path: path.clone(),
        language_id: LanguageId::new("rust").unwrap(),
        source: Arc::<[u8]>::from("fn créé() {}\n".as_bytes()),
        expected_generation: before_generation,
        parse_policy: ParsePolicy::RequireClean,
        create_parents: true,
    };

    let result = service.create_file(request.clone()).expect("create file");
    assert_eq!(
        fs::read_to_string(fixture.root.join(Path::new(path.as_str()))).unwrap(),
        "fn créé() {}\n"
    );
    assert_eq!(
        result.generation,
        Generation::new(before_generation.value() + 1)
    );
    assert!(result.graph_changes.added > 0);

    let mut duplicate = request;
    duplicate.expected_generation = result.generation;
    let error = service
        .create_file(duplicate)
        .expect_err("must not overwrite");
    assert!(
        error.to_string().contains("already exists"),
        "unexpected no-overwrite error: {error}"
    );
}

#[test]
fn create_parent_directories_are_removed_after_pre_rename_recovery() {
    let fixture = Fixture::new("fn before() {}\n");
    let (mut service, _) = fixture.open();
    service.set_fault_injector(Arc::new(FailOnce::new(FaultPoint::AfterTemp)));
    let request = DurableCreateRequest {
        operation_id: "parent-rollback".to_owned(),
        project_id: fixture.project.clone(),
        relative_path: ProjectRelativePath::new("new/only/file.rs").unwrap(),
        language_id: LanguageId::new("rust").unwrap(),
        source: Arc::<[u8]>::from("fn created() {}\n".as_bytes()),
        expected_generation: generation(&fixture.database, &fixture.project),
        parse_policy: ParsePolicy::RequireClean,
        create_parents: true,
    };
    service.create_file(request).expect_err("injected fault");
    drop(service);

    let (_service, recovery) = fixture.open();
    assert!(recovery.entries.iter().all(|entry| entry.resolved));
    assert!(!fixture.root.join("new").exists());
}
