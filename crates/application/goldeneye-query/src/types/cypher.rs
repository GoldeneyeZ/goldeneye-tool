use std::collections::BTreeMap;

use goldeneye_domain::ProjectId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::NodeSummary;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryGraphRequest {
    pub project: ProjectId,
    pub query: String,
    pub max_rows: usize,
}

impl QueryGraphRequest {
    #[must_use]
    pub fn new(project: ProjectId, query: impl Into<String>) -> Self {
        Self {
            project,
            query: query.into(),
            max_rows: 200,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgeSummary {
    pub source_id: String,
    pub target_id: String,
    pub kind: String,
    pub discriminator: String,
    pub generation: u64,
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum QueryValue {
    Null,
    Bool(bool),
    Integer(i64),
    Unsigned(u64),
    Float(f64),
    String(String),
    Node(NodeSummary),
    Edge(EdgeSummary),
    Json(Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryGraphResult {
    pub project: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<QueryValue>>,
    pub total: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}
