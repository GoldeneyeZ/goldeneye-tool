use goldeneye_domain::{FileId, GraphEdge, GraphNode, NodeId, ProjectId, ProjectRelativePath};

use crate::PortError;

/// Durable Git history for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileHistoryRecord {
    pub path: ProjectRelativePath,
    pub change_count: u64,
    pub last_modified: i64,
}

/// Durable co-change relationship between two files.
#[derive(Debug, Clone, PartialEq)]
pub struct GitCoChangeRecord {
    pub file_a: ProjectRelativePath,
    pub file_b: ProjectRelativePath,
    pub co_changes: u64,
    pub coupling_score: f64,
    pub last_co_change: i64,
}

/// Counts produced by one atomic Git-history replacement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitHistoryOutcome {
    pub files: usize,
    pub couplings: usize,
    pub enriched_files: usize,
    pub enriched_edges: usize,
}

/// Git-history persistence and impact-analysis reads required by application services.
pub trait GitHistoryRepository: Send {
    /// Atomically replaces Git history and derived graph enrichment for one project.
    ///
    /// # Errors
    ///
    /// Returns an error when validation or persistence fails.
    fn replace_git_history(
        &mut self,
        project: &ProjectId,
        files: &[GitFileHistoryRecord],
        couplings: &[GitCoChangeRecord],
    ) -> Result<GitHistoryOutcome, PortError>;

    /// Returns file couplings involving `path`.
    ///
    /// # Errors
    ///
    /// Returns an error when the repository query fails.
    fn coupled_files(
        &self,
        project: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<Vec<GitCoChangeRecord>, PortError>;

    /// Returns graph nodes sourced from `file`.
    ///
    /// # Errors
    ///
    /// Returns an error when the repository query fails.
    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError>;

    /// Returns graph edges whose destination is `node`.
    ///
    /// # Errors
    ///
    /// Returns an error when the repository query fails.
    fn edges_to(&self, project: &ProjectId, node: &NodeId) -> Result<Vec<GraphEdge>, PortError>;

    /// Returns one graph node when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error when the repository query fails.
    fn get_node(&self, project: &ProjectId, node: &NodeId) -> Result<Option<GraphNode>, PortError>;
}

impl<T> GitHistoryRepository for Box<T>
where
    T: GitHistoryRepository + ?Sized,
{
    fn replace_git_history(
        &mut self,
        project: &ProjectId,
        files: &[GitFileHistoryRecord],
        couplings: &[GitCoChangeRecord],
    ) -> Result<GitHistoryOutcome, PortError> {
        self.as_mut().replace_git_history(project, files, couplings)
    }

    fn coupled_files(
        &self,
        project: &ProjectId,
        path: &ProjectRelativePath,
    ) -> Result<Vec<GitCoChangeRecord>, PortError> {
        self.as_ref().coupled_files(project, path)
    }

    fn nodes_for_file(&self, file: &FileId) -> Result<Vec<GraphNode>, PortError> {
        self.as_ref().nodes_for_file(file)
    }

    fn edges_to(&self, project: &ProjectId, node: &NodeId) -> Result<Vec<GraphEdge>, PortError> {
        self.as_ref().edges_to(project, node)
    }

    fn get_node(&self, project: &ProjectId, node: &NodeId) -> Result<Option<GraphNode>, PortError> {
        self.as_ref().get_node(project, node)
    }
}
