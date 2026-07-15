use std::{collections::BTreeMap, path::PathBuf};

use goldeneye_domain::ProjectId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error(transparent)]
    Store(#[from] goldeneye_store::StoreError),
    #[error("project is not indexed: {0:?}")]
    ProjectNotFound(ProjectId),
    #[error("invalid regular expression for {field}: {source}")]
    InvalidPattern {
        field: &'static str,
        #[source]
        source: regex::Error,
    },
    #[error("page limit must be between 1 and {maximum}, got {actual}")]
    InvalidPageLimit { actual: usize, maximum: usize },
    #[error("cursor and explicit offset cannot be combined")]
    CursorWithOffset,
    #[error("invalid page cursor")]
    InvalidCursor,
    #[error("page cursor does not match the search filters")]
    CursorMismatch,
    #[error("search has {actual} candidates; maximum is {maximum}")]
    TooManySearchCandidates { actual: u64, maximum: usize },
    #[error("symbol is ambiguous: {query}")]
    AmbiguousSymbol {
        query: String,
        candidates: Vec<NodeSummary>,
    },
    #[error("symbol was not found: {query}")]
    SymbolNotFound {
        query: String,
        suggestions: Vec<NodeSummary>,
    },
    #[error("trace depth must be between 1 and {maximum}, got {actual}")]
    InvalidTraceDepth { actual: usize, maximum: usize },
    #[error("trace limit must be between 1 and {maximum}, got {actual}")]
    InvalidTraceLimit { actual: usize, maximum: usize },
    #[error("source file is missing from the index: {path}")]
    IndexedFileNotFound { path: String },
    #[error("symbol has no indexed file: {qualified_name}")]
    SourceFileUnavailable { qualified_name: String },
    #[error("symbol has no indexed source span: {qualified_name}")]
    SourceSpanUnavailable { qualified_name: String },
    #[error("cannot read source file {path}: {source}")]
    SourceRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("indexed source file is stale: {path}")]
    StaleFile {
        path: String,
        expected_hash: String,
        actual_hash: String,
    },
    #[error("source span is outside file bounds: {qualified_name}")]
    CorruptSourceSpan { qualified_name: String },
    #[error("source span is not valid UTF-8: {qualified_name}")]
    SourceNotUtf8 { qualified_name: String },
    #[error(
        "snippet exceeds bounds: {actual_bytes} bytes/{actual_lines} lines; maximum {maximum_bytes} bytes/{maximum_lines} lines"
    )]
    SnippetTooLarge {
        actual_bytes: usize,
        actual_lines: usize,
        maximum_bytes: usize,
        maximum_lines: usize,
    },
    #[error("{field} must be between 1 and {maximum}, got {actual}")]
    InvalidSnippetLimit {
        field: &'static str,
        actual: usize,
        maximum: usize,
    },
    #[error("mutating Cypher keyword is forbidden: {keyword}")]
    MutatingQuery { keyword: String },
    #[error("unsupported Cypher query: {message}")]
    UnsupportedQuery { message: String },
    #[error("Cypher syntax error at byte {position}: {message}")]
    CypherSyntax { position: usize, message: String },
    #[error("query row limit must be between 1 and {maximum}, got {actual}")]
    InvalidQueryRowLimit { actual: usize, maximum: usize },
    #[error("search pattern must not be empty")]
    EmptySearchPattern,
    #[error("search_code limit must be between 1 and {maximum}, got {actual}")]
    InvalidSearchCodeLimit { actual: usize, maximum: usize },
    #[error("path or file_pattern contains invalid characters")]
    InvalidSearchPathArgument,
    #[error("indexed source path escapes the project root: {path}")]
    SourceOutsideProject { path: PathBuf },
    #[error("semantic search requires between 1 and {maximum} non-empty keywords")]
    InvalidSemanticKeywords { maximum: usize },
    #[error("semantic search limit must be between 1 and {maximum}, got {actual}")]
    InvalidSemanticLimit { actual: usize, maximum: usize },
    #[error("similarity threshold must be between 0 and 1, got {actual}")]
    InvalidSimilarityThreshold { actual: f64 },
    #[error("similarity search limit must be between 1 and {maximum}, got {actual}")]
    InvalidSimilarityLimit { actual: usize, maximum: usize },
    #[error("no persisted structural signature for symbol: {qualified_name}")]
    SignatureNotFound { qualified_name: String },
    #[error("persisted semantic artifact is corrupt: {reason}")]
    CorruptSemanticArtifact { reason: String },
}

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticSearchRequest {
    pub project: ProjectId,
    pub keywords: Vec<String>,
    pub limit: usize,
}

impl SemanticSearchRequest {
    #[must_use]
    pub const fn new(project: ProjectId, keywords: Vec<String>) -> Self {
        Self {
            project,
            keywords,
            limit: 16,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticSearchHit {
    pub node: NodeSummary,
    pub score: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SemanticSearchResult {
    pub project: String,
    pub keyword_count: usize,
    pub results: Vec<SemanticSearchHit>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilaritySearchRequest {
    pub project: ProjectId,
    pub qualified_name: String,
    pub threshold: f64,
    pub limit: usize,
}

impl SimilaritySearchRequest {
    #[must_use]
    pub fn new(project: ProjectId, qualified_name: impl Into<String>) -> Self {
        Self {
            project,
            qualified_name: qualified_name.into(),
            threshold: 0.95,
            limit: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilaritySearchHit {
    pub node: NodeSummary,
    pub similarity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilaritySearchResult {
    pub project: String,
    pub origin: NodeSummary,
    pub results: Vec<SimilaritySearchHit>,
}

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
