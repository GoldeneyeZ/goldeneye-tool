use goldeneye_domain::ProjectId;
use serde::{Deserialize, Serialize};

use super::NodeSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceDirection {
    Inbound,
    Outbound,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TracePathRequest {
    pub project: ProjectId,
    pub function_name: String,
    pub direction: TraceDirection,
    pub depth: usize,
    pub limit: usize,
    pub edge_types: Vec<String>,
}

impl TracePathRequest {
    #[must_use]
    pub fn new(
        project: ProjectId,
        function_name: impl Into<String>,
        direction: TraceDirection,
    ) -> Self {
        Self {
            project,
            function_name: function_name.into(),
            direction,
            depth: 1,
            limit: 200,
            edge_types: vec!["CALLS".to_owned()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceHop {
    pub source_qualified_name: String,
    pub target_qualified_name: String,
    pub related_qualified_name: String,
    pub edge_kind: String,
    pub hop: usize,
    pub file_path: Option<String>,
    pub line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TracePathResult {
    pub project: String,
    pub origin: NodeSummary,
    pub direction: TraceDirection,
    pub paths: Vec<TraceHop>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSnippetRequest {
    pub project: ProjectId,
    pub qualified_name: String,
    pub max_bytes: usize,
    pub max_lines: usize,
}

impl CodeSnippetRequest {
    #[must_use]
    pub fn new(project: ProjectId, qualified_name: impl Into<String>) -> Self {
        Self {
            project,
            qualified_name: qualified_name.into(),
            max_bytes: 64 * 1_024,
            max_lines: 400,
        }
    }
}

pub const DEFAULT_SNIPPET_CHUNK_BYTES: usize = 8_192;
pub const MIN_SNIPPET_CHUNK_BYTES: usize = 256;
pub const MAX_SNIPPET_CHUNK_BYTES: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSnippetManifestRequest {
    pub project: ProjectId,
    pub qualified_name: String,
    pub chunk_bytes: usize,
}

impl CodeSnippetManifestRequest {
    #[must_use]
    pub fn new(project: ProjectId, qualified_name: impl Into<String>) -> Self {
        Self {
            project,
            qualified_name: qualified_name.into(),
            chunk_bytes: DEFAULT_SNIPPET_CHUNK_BYTES,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodeSnippetChunkRequest {
    pub project: ProjectId,
    pub qualified_name: String,
    pub chunk: usize,
    pub chunk_bytes: usize,
    pub expected_source_sha256: Option<String>,
}

impl CodeSnippetChunkRequest {
    #[must_use]
    pub fn new(project: ProjectId, qualified_name: impl Into<String>, chunk: usize) -> Self {
        Self {
            project,
            qualified_name: qualified_name.into(),
            chunk,
            chunk_bytes: DEFAULT_SNIPPET_CHUNK_BYTES,
            expected_source_sha256: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeSnippetResult {
    pub project: String,
    pub symbol: NodeSummary,
    pub source: String,
    pub file_path: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u64,
    pub end_line: u64,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeSnippetManifestResult {
    pub project: String,
    pub symbol: NodeSummary,
    pub file_path: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u64,
    pub end_line: u64,
    pub source_bytes: usize,
    pub source_lines: usize,
    pub source_sha256: String,
    pub indexed_file_hash: String,
    pub chunk_bytes: usize,
    pub chunk_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeSnippetChunkResult {
    pub project: String,
    pub symbol: NodeSummary,
    pub source: String,
    pub file_path: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: u64,
    pub end_line: u64,
    pub source_bytes: usize,
    pub source_lines: usize,
    pub source_sha256: String,
    pub indexed_file_hash: String,
    pub chunk_bytes: usize,
    pub chunk_count: usize,
    pub chunk: usize,
    pub chunk_start_byte: usize,
    pub chunk_end_byte: usize,
    pub file_chunk_start_byte: usize,
    pub file_chunk_end_byte: usize,
    pub chunk_start_line: u64,
    pub chunk_end_line: u64,
    pub eof: bool,
    pub truncated: bool,
}
