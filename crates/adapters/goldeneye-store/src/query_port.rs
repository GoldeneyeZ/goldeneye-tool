use goldeneye_domain::{FileId, FileRecord, GraphEdge, GraphNode, ProjectId, ProjectRecord};
use goldeneye_ports::{
    ConnectionSettings, GraphCounts, NodeSignatureRecord, NodeVectorRecord, PortError,
    QueryRepository, SchemaInfo, SearchHit, TokenVectorRecord,
};

use crate::QueryStore;

impl QueryRepository for QueryStore {
    fn schema_info(&self) -> Result<SchemaInfo, PortError> {
        QueryStore::schema_info(self).map_err(PortError::new)
    }

    fn connection_settings(&self) -> Result<ConnectionSettings, PortError> {
        QueryStore::connection_settings(self).map_err(PortError::new)
    }

    fn get_project(&self, project: &ProjectId) -> Result<Option<ProjectRecord>, PortError> {
        QueryStore::get_project(self, project).map_err(PortError::new)
    }

    fn list_projects(&self) -> Result<Vec<ProjectRecord>, PortError> {
        QueryStore::list_projects(self).map_err(PortError::new)
    }

    fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, PortError> {
        QueryStore::list_files(self, project).map_err(PortError::new)
    }

    fn get_file(&self, file: &FileId) -> Result<Option<FileRecord>, PortError> {
        QueryStore::get_file(self, file).map_err(PortError::new)
    }

    fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, PortError> {
        QueryStore::list_nodes(self, project).map_err(PortError::new)
    }

    fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, PortError> {
        QueryStore::list_edges(self, project).map_err(PortError::new)
    }

    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError> {
        QueryStore::nodes_for_file(self, file).map_err(PortError::new)
    }

    fn search_nodes_page(
        &self,
        project: &ProjectId,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, PortError> {
        QueryStore::search_nodes_page(self, project, query, limit, offset).map_err(PortError::new)
    }

    fn count_search_nodes(&self, project: &ProjectId, query: &str) -> Result<u64, PortError> {
        QueryStore::count_search_nodes(self, project, query).map_err(PortError::new)
    }

    fn list_node_vectors(&self, project: &ProjectId) -> Result<Vec<NodeVectorRecord>, PortError> {
        QueryStore::list_node_vectors(self, project).map_err(PortError::new)
    }

    fn get_token_vector(
        &self,
        project: &ProjectId,
        token: &str,
    ) -> Result<Option<TokenVectorRecord>, PortError> {
        QueryStore::get_token_vector(self, project, token).map_err(PortError::new)
    }

    fn list_node_signatures(
        &self,
        project: &ProjectId,
    ) -> Result<Vec<NodeSignatureRecord>, PortError> {
        QueryStore::list_node_signatures(self, project).map_err(PortError::new)
    }

    fn get_node_signature(
        &self,
        project: &ProjectId,
        node: &goldeneye_domain::NodeId,
    ) -> Result<Option<NodeSignatureRecord>, PortError> {
        QueryStore::get_node_signature(self, project, node).map_err(PortError::new)
    }

    fn counts(&self, project: &ProjectId) -> Result<GraphCounts, PortError> {
        QueryStore::counts(self, project).map_err(PortError::new)
    }
}
