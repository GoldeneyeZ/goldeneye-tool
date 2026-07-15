use std::collections::BTreeMap;

use goldeneye_domain::ProjectId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub project: String,
    pub root_path: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStatusRequest {
    pub project: ProjectId,
}

impl IndexStatusRequest {
    #[must_use]
    pub const fn new(project: ProjectId) -> Self {
        Self { project }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStatusResult {
    pub project: String,
    pub root_path: String,
    pub generation: u64,
    pub files: u64,
    pub nodes: u64,
    pub edges: u64,
    pub query_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSchemaRequest {
    pub project: ProjectId,
}

impl GraphSchemaRequest {
    #[must_use]
    pub const fn new(project: ProjectId) -> Self {
        Self { project }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaEntry {
    pub name: String,
    pub count: u64,
    pub properties: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSchemaResult {
    pub project: String,
    pub schema_version: u32,
    pub node_labels: Vec<SchemaEntry>,
    pub edge_types: Vec<SchemaEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    pub limit: usize,
    pub offset: usize,
    pub cursor: Option<String>,
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: 20,
            offset: 0,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchGraphRequest {
    pub project: ProjectId,
    pub query: Option<String>,
    pub name_pattern: Option<String>,
    pub qualified_name_pattern: Option<String>,
    pub label: Option<String>,
    pub file_pattern: Option<String>,
    pub relationship: Option<String>,
    pub min_degree: Option<usize>,
    pub max_degree: Option<usize>,
    pub exclude_entry_points: bool,
    pub include_connected: bool,
    pub page: PageRequest,
}

impl SearchGraphRequest {
    #[must_use]
    pub fn new(project: ProjectId) -> Self {
        Self {
            project,
            query: None,
            name_pattern: None,
            qualified_name_pattern: None,
            label: None,
            file_pattern: None,
            relationship: None,
            min_degree: None,
            max_degree: None,
            exclude_entry_points: false,
            include_connected: false,
            page: PageRequest::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NodeSummary {
    pub id: String,
    pub name: String,
    pub qualified_name: String,
    pub label: String,
    pub file_path: Option<String>,
    pub start_byte: Option<u64>,
    pub end_byte: Option<u64>,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
    pub generation: u64,
    pub in_degree: usize,
    pub out_degree: usize,
    pub rank: Option<f64>,
    pub connected_names: Vec<String>,
    pub properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchGraphPage {
    pub project: String,
    pub results: Vec<NodeSummary>,
    pub total: usize,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchCodeMode {
    #[default]
    Compact,
    Full,
    Files,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCodeRequest {
    pub project: ProjectId,
    pub pattern: String,
    pub file_pattern: Option<String>,
    pub path_filter: Option<String>,
    pub mode: SearchCodeMode,
    pub context: usize,
    pub regex: bool,
    pub limit: usize,
}

impl SearchCodeRequest {
    #[must_use]
    pub fn new(project: ProjectId, pattern: impl Into<String>) -> Self {
        Self {
            project,
            pattern: pattern.into(),
            file_pattern: None,
            path_filter: None,
            mode: SearchCodeMode::Compact,
            context: 0,
            regex: false,
            limit: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCodeHit {
    pub node: String,
    pub qualified_name: String,
    pub label: String,
    pub file: String,
    pub start_line: u64,
    pub end_line: u64,
    pub in_degree: usize,
    pub out_degree: usize,
    pub match_lines: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_start: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_start: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawCodeMatch {
    pub file: String,
    pub line: u64,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCodeMatchesResult {
    pub results: Vec<SearchCodeHit>,
    pub raw_matches: Vec<RawCodeMatch>,
    pub directories: BTreeMap<String, usize>,
    pub total_grep_matches: usize,
    pub total_results: usize,
    pub raw_match_count: usize,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup_ratio: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchCodeFilesResult {
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SearchCodeResult {
    Matches(SearchCodeMatchesResult),
    Files(SearchCodeFilesResult),
}
