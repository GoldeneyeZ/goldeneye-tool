use goldeneye_domain::{
    FileId, FileRecord, Generation, GraphEdge, GraphNode, NodeId, ProjectId, ProjectRecord,
};

use crate::{CrossLinkRepository, GraphCounts, PortError};

/// Persistence operations required by repository indexing.
pub trait IndexRepository: CrossLinkRepository + Send {
    /// Reads one indexed project.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read the project.
    fn get_project(&self, project: &ProjectId) -> Result<Option<ProjectRecord>, PortError>;

    /// Lists the files currently stored for `project`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read the project's files.
    fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, PortError>;

    /// Counts the persisted graph records for `project`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot count the graph.
    fn counts(&self, project: &ProjectId) -> Result<GraphCounts, PortError>;

    /// Lists all graph nodes owned by `file`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read the file graph.
    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError>;

    /// Reads one graph node from `project`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read the node.
    fn get_node(&self, project: &ProjectId, node: &NodeId) -> Result<Option<GraphNode>, PortError>;

    /// Lists the graph edges whose source is `node`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read the node's edges.
    fn edges_from(&self, project: &ProjectId, node: &NodeId) -> Result<Vec<GraphEdge>, PortError>;

    /// Atomically replaces a project's complete file graph and returns its new generation.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails. Implementations must not expose a
    /// partially replaced graph.
    fn replace_project_graph(
        &mut self,
        project: &ProjectRecord,
        files: Vec<FileRecord>,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    ) -> Result<Generation, PortError>;
}

impl<T> IndexRepository for Box<T>
where
    T: IndexRepository + ?Sized,
{
    fn get_project(&self, project: &ProjectId) -> Result<Option<ProjectRecord>, PortError> {
        self.as_ref().get_project(project)
    }

    fn list_files(&self, project: &ProjectId) -> Result<Vec<FileRecord>, PortError> {
        self.as_ref().list_files(project)
    }

    fn counts(&self, project: &ProjectId) -> Result<GraphCounts, PortError> {
        self.as_ref().counts(project)
    }

    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError> {
        self.as_ref().nodes_for_file(file)
    }

    fn get_node(&self, project: &ProjectId, node: &NodeId) -> Result<Option<GraphNode>, PortError> {
        self.as_ref().get_node(project, node)
    }

    fn edges_from(&self, project: &ProjectId, node: &NodeId) -> Result<Vec<GraphEdge>, PortError> {
        self.as_ref().edges_from(project, node)
    }

    fn replace_project_graph(
        &mut self,
        project: &ProjectRecord,
        files: Vec<FileRecord>,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    ) -> Result<Generation, PortError> {
        self.as_mut()
            .replace_project_graph(project, files, nodes, edges)
    }
}
