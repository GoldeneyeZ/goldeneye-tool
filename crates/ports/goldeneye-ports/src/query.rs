use std::collections::BTreeSet;

use goldeneye_domain::{
    FileId, FileRecord, GraphEdge, GraphNode, NodeId, ProjectId, ProjectRecord,
};

use crate::PortError;

pub const STORED_VECTOR_DIM: usize = 768;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchemaInfo {
    pub version: u32,
    pub tables: BTreeSet<String>,
    pub indexes: BTreeSet<String>,
    pub fts5_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionSettings {
    pub foreign_keys: bool,
    pub journal_mode: String,
    pub synchronous: i64,
    pub busy_timeout_ms: u64,
    pub query_only: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphCounts {
    pub files: u64,
    pub nodes: u64,
    pub edges: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVector([i8; STORED_VECTOR_DIM]);

impl StoredVector {
    #[must_use]
    pub const fn from_array(values: [i8; STORED_VECTOR_DIM]) -> Self {
        Self(values)
    }

    #[must_use]
    pub const fn values(&self) -> &[i8; STORED_VECTOR_DIM] {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVectorRecord {
    pub node_id: NodeId,
    pub vector: StoredVector,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenVectorRecord {
    pub token: String,
    pub vector: StoredVector,
    /// Inverse-document frequency multiplied by 1,000, matching upstream storage.
    pub idf_milli: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSignatureRecord {
    pub node_id: NodeId,
    pub minhash_hex: String,
    pub ast_profile: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub node: GraphNode,
    pub rank: f64,
}

/// Read operations required by Goldeneye query use cases.
pub trait QueryRepository: Send {
    /// Returns durable schema metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when schema metadata cannot be read.
    fn schema_info(&self) -> Result<SchemaInfo, PortError>;

    /// Returns effective read-connection settings.
    ///
    /// # Errors
    ///
    /// Returns an error when connection settings cannot be read.
    fn connection_settings(&self) -> Result<ConnectionSettings, PortError>;

    /// Finds one indexed project.
    ///
    /// # Errors
    ///
    /// Returns an error when the project registry cannot be read.
    fn get_project(&self, project: &ProjectId) -> Result<Option<ProjectRecord>, PortError>;

    /// Lists indexed projects in stable ID order.
    ///
    /// # Errors
    ///
    /// Returns an error when the project registry cannot be read.
    fn list_projects(&self) -> Result<Vec<ProjectRecord>, PortError>;

    /// Lists files belonging to one project.
    ///
    /// # Errors
    ///
    /// Returns an error when file records cannot be read.
    fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, PortError>;

    /// Finds one indexed file by stable ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the file record cannot be read.
    fn get_file(&self, file: &FileId) -> Result<Option<FileRecord>, PortError>;

    /// Lists one project's graph nodes.
    ///
    /// # Errors
    ///
    /// Returns an error when graph nodes cannot be read.
    fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, PortError>;

    /// Lists one project's graph edges.
    ///
    /// # Errors
    ///
    /// Returns an error when graph edges cannot be read.
    fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, PortError>;

    /// Lists graph nodes originating from one file.
    ///
    /// # Errors
    ///
    /// Returns an error when graph nodes cannot be read.
    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError>;

    /// Returns one page of full-text node matches.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid full-text syntax or repository read failures.
    fn search_nodes_page(
        &self,
        project: &ProjectId,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, PortError>;

    /// Counts full-text node matches without materializing them.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid full-text syntax or repository read failures.
    fn count_search_nodes(&self, project: &ProjectId, query: &str) -> Result<u64, PortError>;

    /// Lists persisted semantic vectors for graph nodes.
    ///
    /// # Errors
    ///
    /// Returns an error when semantic vectors cannot be read or decoded.
    fn list_node_vectors(&self, project: &ProjectId) -> Result<Vec<NodeVectorRecord>, PortError>;

    /// Finds one persisted semantic token vector.
    ///
    /// # Errors
    ///
    /// Returns an error when the token vector cannot be read or decoded.
    fn get_token_vector(
        &self,
        project: &ProjectId,
        token: &str,
    ) -> Result<Option<TokenVectorRecord>, PortError>;

    /// Lists persisted structural signatures for graph nodes.
    ///
    /// # Errors
    ///
    /// Returns an error when signatures cannot be read or decoded.
    fn list_node_signatures(
        &self,
        project: &ProjectId,
    ) -> Result<Vec<NodeSignatureRecord>, PortError>;

    /// Finds one persisted structural signature by node ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the signature cannot be read or decoded.
    fn get_node_signature(
        &self,
        project: &ProjectId,
        node: &NodeId,
    ) -> Result<Option<NodeSignatureRecord>, PortError>;

    /// Returns aggregate graph counts for one project.
    ///
    /// # Errors
    ///
    /// Returns an error when graph counts cannot be read.
    fn counts(&self, project: &ProjectId) -> Result<GraphCounts, PortError>;
}

impl<T> QueryRepository for Box<T>
where
    T: QueryRepository + ?Sized,
{
    fn schema_info(&self) -> Result<SchemaInfo, PortError> {
        self.as_ref().schema_info()
    }

    fn connection_settings(&self) -> Result<ConnectionSettings, PortError> {
        self.as_ref().connection_settings()
    }

    fn get_project(&self, project: &ProjectId) -> Result<Option<ProjectRecord>, PortError> {
        self.as_ref().get_project(project)
    }

    fn list_projects(&self) -> Result<Vec<ProjectRecord>, PortError> {
        self.as_ref().list_projects()
    }

    fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, PortError> {
        self.as_ref().list_files(project)
    }

    fn get_file(&self, file: &FileId) -> Result<Option<FileRecord>, PortError> {
        self.as_ref().get_file(file)
    }

    fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, PortError> {
        self.as_ref().list_nodes(project)
    }

    fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, PortError> {
        self.as_ref().list_edges(project)
    }

    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError> {
        self.as_ref().nodes_for_file(file)
    }

    fn search_nodes_page(
        &self,
        project: &ProjectId,
        query: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<SearchHit>, PortError> {
        self.as_ref()
            .search_nodes_page(project, query, limit, offset)
    }

    fn count_search_nodes(&self, project: &ProjectId, query: &str) -> Result<u64, PortError> {
        self.as_ref().count_search_nodes(project, query)
    }

    fn list_node_vectors(&self, project: &ProjectId) -> Result<Vec<NodeVectorRecord>, PortError> {
        self.as_ref().list_node_vectors(project)
    }

    fn get_token_vector(
        &self,
        project: &ProjectId,
        token: &str,
    ) -> Result<Option<TokenVectorRecord>, PortError> {
        self.as_ref().get_token_vector(project, token)
    }

    fn list_node_signatures(
        &self,
        project: &ProjectId,
    ) -> Result<Vec<NodeSignatureRecord>, PortError> {
        self.as_ref().list_node_signatures(project)
    }

    fn get_node_signature(
        &self,
        project: &ProjectId,
        node: &NodeId,
    ) -> Result<Option<NodeSignatureRecord>, PortError> {
        self.as_ref().get_node_signature(project, node)
    }

    fn counts(&self, project: &ProjectId) -> Result<GraphCounts, PortError> {
        self.as_ref().counts(project)
    }
}
