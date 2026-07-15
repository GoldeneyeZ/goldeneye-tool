use goldeneye_domain::{FileId, GraphEdge, GraphNode, NodeId, ProjectId, ProjectRelativePath};
use goldeneye_ports::{
    GitCoChangeRecord, GitFileHistoryRecord, GitHistoryOutcome, GitHistoryRepository, PortError,
};

use crate::Store;

impl GitHistoryRepository for Store {
    fn replace_git_history(
        &mut self,
        project: &ProjectId,
        files: &[GitFileHistoryRecord],
        couplings: &[GitCoChangeRecord],
    ) -> Result<GitHistoryOutcome, PortError> {
        Store::replace_git_history(self, project, files, couplings).map_err(PortError::new)
    }

    fn coupled_files(
        &self,
        project: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<Vec<GitCoChangeRecord>, PortError> {
        Store::coupled_files(self, project, path).map_err(PortError::new)
    }

    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError> {
        Store::nodes_for_file(self, file).map_err(PortError::new)
    }

    fn edges_to(&self, project: &ProjectId, node: &NodeId) -> Result<Vec<GraphEdge>, PortError> {
        Store::edges_to(self, project, node).map_err(PortError::new)
    }

    fn get_node(&self, project: &ProjectId, node: &NodeId) -> Result<Option<GraphNode>, PortError> {
        Store::get_node(self, project, node).map_err(PortError::new)
    }
}
