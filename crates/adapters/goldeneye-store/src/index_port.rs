use goldeneye_domain::{
    FileId, FileRecord, Generation, GraphEdge, GraphNode, NodeId, ProjectId, ProjectRecord,
};
use goldeneye_ports::{GraphCounts, IndexRepository, PortError};

use crate::Store;

impl IndexRepository for Store {
    fn get_project(&self, project: &ProjectId) -> Result<Option<ProjectRecord>, PortError> {
        Store::get_project(self, project).map_err(PortError::new)
    }

    fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, PortError> {
        Store::list_files(self, project).map_err(PortError::new)
    }

    fn counts(&self, project: &ProjectId) -> Result<GraphCounts, PortError> {
        Store::counts(self, project).map_err(PortError::new)
    }

    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError> {
        Store::nodes_for_file(self, file).map_err(PortError::new)
    }

    fn get_node(&self, project: &ProjectId, node: &NodeId) -> Result<Option<GraphNode>, PortError> {
        Store::get_node(self, project, node).map_err(PortError::new)
    }

    fn edges_from(&self, project: &ProjectId, node: &NodeId) -> Result<Vec<GraphEdge>, PortError> {
        Store::edges_from(self, project, node).map_err(PortError::new)
    }

    fn replace_project_graph(
        &mut self,
        project: &ProjectRecord,
        files: Vec<FileRecord>,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    ) -> Result<Generation, PortError> {
        Store::replace_project_graph(self, project, files, nodes, edges)
            .map(|outcome| outcome.generation)
            .map_err(PortError::new)
    }
}
