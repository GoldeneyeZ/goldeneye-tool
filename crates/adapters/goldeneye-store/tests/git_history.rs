use goldeneye_domain::{
    ContentHash, FileId, FileRecord, Generation, GraphNode, NodeId, NodeLabel, ProjectId,
    ProjectRecord, ProjectRelativePath, QualifiedName,
};
use goldeneye_store::{GitCoChangeRecord, GitFileHistoryRecord, Store};

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

fn file_node(project: &ProjectId, path: &str, id: &str) -> GraphNode {
    GraphNode::new(
        project.clone(),
        NodeId::new(id).expect("node id"),
        NodeLabel::new("File").expect("label"),
        path,
        QualifiedName::new(format!("demo.{path}.__file__")).expect("qualified name"),
        Some(rel(path)),
        None,
        Generation::new(0),
    )
    .expect("file node")
}

#[test]
fn history_snapshot_is_durable_and_enriches_file_graph() {
    let mut store = Store::open_in_memory().expect("store");
    let project_id = ProjectId::new("demo").expect("project id");
    let project = ProjectRecord::new(project_id.clone(), "/repo").expect("project");
    store
        .replace_project_graph(
            &project,
            vec![file(&project_id, "a.rs"), file(&project_id, "b.rs")],
            vec![
                file_node(&project_id, "a.rs", "file:a"),
                file_node(&project_id, "b.rs", "file:b"),
            ],
            vec![],
        )
        .expect("graph");

    let outcome = store
        .replace_git_history(
            &project_id,
            &[
                GitFileHistoryRecord {
                    path: rel("a.rs"),
                    change_count: 4,
                    last_modified: 100,
                },
                GitFileHistoryRecord {
                    path: rel("b.rs"),
                    change_count: 3,
                    last_modified: 90,
                },
            ],
            &[GitCoChangeRecord {
                file_a: rel("a.rs"),
                file_b: rel("b.rs"),
                co_changes: 3,
                coupling_score: 1.0,
                last_co_change: 90,
            }],
        )
        .expect("history");
    assert_eq!((outcome.enriched_files, outcome.enriched_edges), (2, 1));
    assert_eq!(
        store
            .list_git_file_history(&project_id)
            .expect("files")
            .len(),
        2
    );
    assert_eq!(
        store
            .coupled_files(&project_id, &rel("a.rs"))
            .expect("coupled")
            .len(),
        1
    );

    let nodes = store.list_nodes(&project_id).expect("nodes");
    assert_eq!(nodes[0].properties["change_count"], 4);
    let edges = store.list_edges(&project_id).expect("edges");
    assert_eq!(edges[0].kind.as_str(), "FILE_CHANGES_WITH");
    assert_eq!(edges[0].properties["co_changes"], 3);

    store
        .replace_git_history(&project_id, &[], &[])
        .expect("clear history");
    assert!(
        store
            .list_git_cochanges(&project_id)
            .expect("couplings")
            .is_empty()
    );
    assert!(store.list_edges(&project_id).expect("edges").is_empty());
    assert!(
        !store.list_nodes(&project_id).expect("nodes")[0]
            .properties
            .contains_key("change_count")
    );
}
