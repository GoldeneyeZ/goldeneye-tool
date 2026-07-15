use goldeneye_domain::{GraphEdge, GraphNode, ProjectId, ProjectRecord};

use crate::PortError;

/// Persistence operations required by cross-project edge derivation.
pub trait CrossLinkRepository {
    /// Lists every project available for cross-project analysis.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read its project catalog.
    fn list_projects(&self) -> Result<Vec<ProjectRecord>, PortError>;

    /// Lists the graph nodes stored for `project`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read the project's nodes.
    fn list_nodes(&self, project: &ProjectId) -> Result<Vec<GraphNode>, PortError>;

    /// Lists the graph edges stored for `project`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot read the project's edges.
    fn list_edges(&self, project: &ProjectId) -> Result<Vec<GraphEdge>, PortError>;

    /// Replaces the cross-project edges owned by `project` and returns the inserted count.
    ///
    /// # Errors
    ///
    /// Returns an error when the backing repository cannot persist the replacement edges.
    fn replace_cross_project_edges(
        &mut self,
        project: &ProjectId,
        edges: &[GraphEdge],
    ) -> Result<usize, PortError>;
}
