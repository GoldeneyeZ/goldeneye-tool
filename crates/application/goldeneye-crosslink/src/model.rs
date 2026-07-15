use goldeneye_domain::{GraphEdge, GraphNode, NodeId, ProjectRecord};

pub(super) struct ProjectGraph {
    pub(super) record: ProjectRecord,
    pub(super) nodes: Vec<GraphNode>,
    pub(super) edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone)]
pub(super) struct Endpoint {
    pub(super) project: String,
    pub(super) route_id: NodeId,
    pub(super) handler_id: NodeId,
    pub(super) handler_name: String,
    pub(super) handler_file: String,
}
