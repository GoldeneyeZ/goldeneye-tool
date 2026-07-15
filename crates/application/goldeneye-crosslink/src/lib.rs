//! Cross-project graph enrichment for Goldeneye.

use goldeneye_domain::GraphIdentityError;
use goldeneye_ports::PortError;
use thiserror::Error;

mod edges;
mod engine;
mod model;
mod registry;

pub use engine::rebuild;

const MAX_CROSS_EDGES_PER_PROJECT: usize = 100_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CrossLinkOutcome {
    pub projects: usize,
    pub edges: usize,
}

#[derive(Debug, Error)]
pub enum CrossLinkError {
    #[error(transparent)]
    Repository(#[from] PortError),
    #[error(transparent)]
    Identity(#[from] GraphIdentityError),
    #[error("cross-project edge limit exceeded for {project}: {limit}")]
    EdgeLimit { project: String, limit: usize },
}

#[cfg(test)]
mod tests {
    use goldeneye_domain::{
        EdgeDiscriminator, EdgeKind, Generation, GraphEdge, GraphNode, NodeId, NodeLabel,
        ProjectId, ProjectRecord, QualifiedName,
    };
    use goldeneye_store::Store;

    use super::{edges::deduplicate_edges, rebuild};

    #[test]
    fn single_project_rebuild_clears_stale_cross_edges_without_loading_graphs() {
        let mut store = Store::open_in_memory().expect("store");
        let project_id = ProjectId::new("api").expect("project ID");
        let project = ProjectRecord::new(project_id.clone(), "/api").expect("project");
        let source = GraphNode::new(
            project_id.clone(),
            NodeId::new("source").expect("source ID"),
            NodeLabel::new("Function").expect("source label"),
            "source",
            QualifiedName::new("api.source").expect("source qualified name"),
            None,
            None,
            Generation::new(0),
        )
        .expect("source node");
        let target = GraphNode::new(
            project_id.clone(),
            NodeId::new("target").expect("target ID"),
            NodeLabel::new("Function").expect("target label"),
            "target",
            QualifiedName::new("api.target").expect("target qualified name"),
            None,
            None,
            Generation::new(0),
        )
        .expect("target node");
        let replacement = store
            .replace_project_graph(&project, vec![], vec![source, target], vec![])
            .expect("replace project graph");
        let stale = GraphEdge::new(
            project_id.clone(),
            NodeId::new("source").expect("source ID"),
            NodeId::new("target").expect("target ID"),
            EdgeKind::new("CROSS_HTTP_CALLS").expect("cross edge kind"),
            replacement.generation,
        );
        store
            .replace_cross_project_edges(&project_id, &[stale])
            .expect("seed stale cross edge");

        let outcome = rebuild(&mut store).expect("rebuild");

        assert_eq!(outcome.projects, 1);
        assert_eq!(outcome.edges, 0);
        assert!(
            store
                .list_edges(&project_id)
                .expect("list edges")
                .is_empty()
        );
    }

    #[test]
    fn duplicate_cross_edges_are_collapsed_by_identity() {
        let project = ProjectId::new("api").expect("project");
        let source = NodeId::new("handler").expect("source");
        let target = NodeId::new("route").expect("target");
        let kind = EdgeKind::new("CROSS_HTTP_CALLS").expect("kind");
        let mut first = GraphEdge::new(
            project.clone(),
            source.clone(),
            target.clone(),
            kind.clone(),
            Generation::new(1),
        );
        first.discriminator = EdgeDiscriminator::new("client").expect("discriminator");
        let mut second = GraphEdge::new(project, source, target, kind, Generation::new(1));
        second.discriminator = EdgeDiscriminator::new("client").expect("discriminator");
        second
            .properties
            .insert("target_name".to_owned(), "other_caller".into());
        let mut edges = vec![first, second];

        deduplicate_edges(&mut edges);

        assert_eq!(edges.len(), 1);
    }
}
