use std::path::PathBuf;

use goldeneye_domain::ProjectId;
use thiserror::Error;

use super::NodeSummary;

#[derive(Debug, Error)]
pub enum QueryError {
    #[error(transparent)]
    Repository(#[from] goldeneye_ports::PortError),
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
