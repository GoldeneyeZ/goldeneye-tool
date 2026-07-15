use goldeneye_domain::{GraphEdge, GraphNode, ProjectId, ProjectRecord};
use goldeneye_ports::{CrossLinkRepository, PortError};

use crate::Store;

impl CrossLinkRepository for Store {
    fn list_projects(&self) -> Result<Vec<ProjectRecord>, PortError> {
        Store::list_projects(self).map_err(PortError::new)
    }

    fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, PortError> {
        Store::list_nodes(self, project).map_err(PortError::new)
    }

    fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, PortError> {
        Store::list_edges(self, project).map_err(PortError::new)
    }

    fn replace_cross_project_edges(
        &mut self,
        project: &ProjectId,
        edges: &[GraphEdge],
    ) -> Result<usize, PortError> {
        Store::replace_cross_project_edges(self, project, edges).map_err(PortError::new)
    }
}
