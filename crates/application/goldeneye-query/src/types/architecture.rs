use goldeneye_domain::ProjectId;
use serde::{Deserialize, Serialize};

use super::NodeSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureRequest {
    pub project: ProjectId,
}

impl ArchitectureRequest {
    #[must_use]
    pub const fn new(project: ProjectId) -> Self {
        Self { project }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountSummary {
    pub name: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureModule {
    pub name: String,
    pub qualified_name: String,
    pub file_path: Option<String>,
    pub defined_symbols: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArchitectureResult {
    pub project: String,
    pub root_path: String,
    pub generation: u64,
    pub total_nodes: usize,
    pub total_edges: usize,
    pub languages: Vec<CountSummary>,
    pub modules: Vec<ArchitectureModule>,
    pub types: Vec<NodeSummary>,
    pub entry_points: Vec<NodeSummary>,
    pub edge_types: Vec<CountSummary>,
}
