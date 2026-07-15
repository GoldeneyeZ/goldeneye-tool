use goldeneye_domain::{Generation, ProjectId, ProjectRecord};
use goldeneye_ports::{
    CrossLinkRepository, EditRepository, IndexRepository, ProjectAdministrationRepository,
    QueryRepository, RepositoryFactory,
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
    let mut administration = factory
        .open_project_administration(&database)
        .expect("open project administration repository");
    assert!(delete_project(&mut administration, &project_id));
    assert!(!delete_project(&mut administration, &project_id));
    assert_eq!(query_project_count(&query), 0);
}
