use goldeneye_domain::{FileId, Generation, NodeId, ProjectId, ProjectRecord, ProjectRelativePath};
use goldeneye_ports::{
    AdrTraceRepository, CrossLinkRepository, EditRepository, GitHistoryOutcome,
    GitHistoryRepository, IndexRepository, ProjectAdministrationRepository, QueryRepository,
    RepositoryFactory, RuntimeTraceObservation, SemanticIndexRepository, StoredVector,
    TokenVectorRecord,
};
use goldeneye_store::SqliteRepositoryFactory;

fn query_project_count(repository: &impl QueryRepository) -> usize {
    repository.list_projects().expect("query projects").len()
}

fn indexed_generation(repository: &impl IndexRepository, project: &ProjectId) -> Generation {
    repository
        .get_project(project)
        .expect("indexed project query")
        .expect("indexed project")
        .generation
}

fn edited_generation(repository: &impl EditRepository, project: &ProjectId) -> Generation {
    repository
        .get_project(project)
        .expect("edited project query")
        .expect("edited project")
        .generation
}

fn crosslink_project_count(repository: &impl CrossLinkRepository) -> usize {
    repository
        .list_projects()
        .expect("crosslink projects")
        .len()
}

fn delete_project(
    repository: &mut impl ProjectAdministrationRepository,
    project: &ProjectId,
) -> bool {
    repository.delete_project(project).expect("delete project")
}

fn roundtrip_adr(repository: &mut impl AdrTraceRepository, project: &ProjectId) {
    repository
        .store_adr(project, "## PURPOSE\nFactory port")
        .expect("store ADR");
    assert_eq!(
        repository
            .get_adr(project)
            .expect("read ADR")
            .expect("ADR")
            .content,
        "## PURPOSE\nFactory port"
    );
    assert_eq!(
        repository
            .ingest_runtime_traces(
                project,
                &[
                    RuntimeTraceObservation {
                        caller: "caller".to_owned(),
                        callee: "callee".to_owned(),
                        count: 2,
                    },
                    RuntimeTraceObservation {
                        caller: "caller".to_owned(),
                        callee: "callee".to_owned(),
                        count: 3,
                    },
                ],
            )
            .expect("ingest runtime traces"),
        2
    );
}

fn replace_semantic_tokens(
    repository: &mut impl SemanticIndexRepository,
    project: &ProjectId,
    generation: Generation,
    token_vectors: &[TokenVectorRecord],
) {
    repository
        .replace_semantic_index(project, generation, &[], token_vectors, &[])
        .expect("replace semantic index");
}

fn exercise_git_history_box(
    repository: &mut impl GitHistoryRepository,
    project: &ProjectId,
) -> GitHistoryOutcome {
    let path = ProjectRelativePath::new("src/lib.rs").expect("project-relative path");
    let node = NodeId::new("missing").expect("node ID");
    let outcome = repository
        .replace_git_history(project, &[], &[])
        .expect("replace Git history");
    assert!(
        repository
            .coupled_files(project, &path)
            .expect("query couplings")
            .is_empty()
    );
    assert!(
        repository
            .nodes_for_file(&FileId::new(project.clone(), path))
            .expect("query file nodes")
            .is_empty()
    );
    assert!(
        repository
            .edges_to(project, &node)
            .expect("query inbound edges")
            .is_empty()
    );
    assert!(
        repository
            .get_node(project, &node)
            .expect("query node")
            .is_none()
    );
    outcome
}

#[test]
fn query_open_never_creates_a_missing_database_and_initialize_is_idempotent() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let database = temp.path().join("graph.db");
    let factory = SqliteRepositoryFactory;

    let Err(error) = factory.open_query(&database) else {
        panic!("missing query database must fail");
    };
    assert!(error.to_string().contains("database does not exist"));
    assert!(!database.exists());

    factory.initialize(&database).expect("initialize database");
    factory
        .initialize(&database)
        .expect("reinitialize database");
    let query = factory
        .open_query(&database)
        .expect("open query repository");
    let settings = query.connection_settings().expect("connection settings");

    assert!(settings.query_only);
    assert_eq!(query_project_count(&query), 0);
}

#[test]
fn factory_boxes_forward_repository_ports_and_edit_opens_are_independent() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let database = temp.path().join("graph.db");
    let factory = SqliteRepositoryFactory;
    factory.initialize(&database).expect("initialize database");
    let project_id = ProjectId::new("demo").expect("project ID");
    let project = ProjectRecord::new(project_id.clone(), temp.path().to_string_lossy())
        .expect("project record");

    let mut index = factory
        .open_index(&database)
        .expect("open index repository");
    let generation = index
        .replace_project_graph(&project, Vec::new(), Vec::new(), Vec::new())
        .expect("replace project graph");
    assert_eq!(generation, Generation::new(1));
    assert_eq!(indexed_generation(&index, &project_id), generation);

    let first_edit = factory.open_edit(&database).expect("first edit repository");
    let second_edit = factory
        .open_edit(&database)
        .expect("second edit repository");
    assert_eq!(edited_generation(&first_edit, &project_id), generation);
    assert_eq!(edited_generation(&second_edit, &project_id), generation);

    let crosslink = factory
        .open_crosslink(&database)
        .expect("open crosslink repository");
    assert_eq!(crosslink_project_count(&crosslink), 1);
    let query = factory
        .open_query(&database)
        .expect("open query repository");
    assert_eq!(query_project_count(&query), 1);
    let token = TokenVectorRecord {
        token: "factory".to_owned(),
        vector: StoredVector::from_array([1_i8; 768]),
        idf_milli: 1_000,
    };
    let mut semantic = factory
        .open_semantic_index(&database)
        .expect("open semantic repository");
    replace_semantic_tokens(
        &mut semantic,
        &project_id,
        generation,
        std::slice::from_ref(&token),
    );
    assert_eq!(
        query
            .get_token_vector(&project_id, "factory")
            .expect("warm query sees semantic write"),
        Some(token)
    );
    replace_semantic_tokens(&mut semantic, &project_id, generation, &[]);
    assert_eq!(
        query
            .get_token_vector(&project_id, "factory")
            .expect("warm query sees semantic clear"),
        None
    );
    let mut adr_traces = factory
        .open_adr_traces(&database)
        .expect("open ADR/runtime repository");
    roundtrip_adr(&mut adr_traces, &project_id);
    let mut git_history = factory
        .open_git_history(&database)
        .expect("open Git-history repository");
    assert_eq!(
        exercise_git_history_box(&mut git_history, &project_id),
        GitHistoryOutcome::default()
    );
    let mut administration = factory
        .open_project_administration(&database)
        .expect("open project administration repository");
    assert!(delete_project(&mut administration, &project_id));
    assert!(!delete_project(&mut administration, &project_id));
    assert_eq!(query_project_count(&query), 0);
    let error = adr_traces
        .ingest_runtime_traces(
            &project_id,
            &[RuntimeTraceObservation {
                caller: String::new(),
                callee: String::new(),
                count: 0,
            }],
        )
        .expect_err("missing project must precede trace validation");
    assert!(error.to_string().contains("project not found"), "{error}");
}
