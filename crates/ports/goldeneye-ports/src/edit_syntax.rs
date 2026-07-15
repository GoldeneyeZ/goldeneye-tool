use std::error::Error;
use std::fmt;
use std::sync::Arc;

use goldeneye_domain::{
    AncestorStep, ByteSpan, ContentHash, FileContext, Generation, LanguageId, LocatorScope,
    NodeLocator, SourceSpan,
};
use serde::{Serialize, Serializer};

use crate::PortError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditDiagnosticKind {
    Error,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSyntaxDiagnostic {
    pub kind: EditDiagnosticKind,
    pub node_kind: String,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditInspectRequest {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub preview_chars: usize,
    pub byte_range: Option<ByteSpan>,
    pub node_kinds: Vec<String>,
}

impl Default for EditInspectRequest {
    fn default() -> Self {
        Self {
            max_depth: 4,
            max_nodes: 64,
            preview_chars: 0,
            byte_range: None,
            node_kinds: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditSyntaxNodeView {
    #[serde(rename = "o")]
    pub ordinal: u32,
    #[serde(rename = "p")]
    pub parent_ordinal: Option<u32>,
    #[serde(rename = "d")]
    pub depth: usize,
    #[serde(rename = "i", skip_serializing_if = "Option::is_none")]
    pub named_child_index: Option<u32>,
    #[serde(rename = "f", skip_serializing_if = "Option::is_none")]
    pub field_name: Option<String>,
    #[serde(rename = "k")]
    pub kind: String,
    #[serde(rename = "s", serialize_with = "serialize_source_span")]
    pub span: SourceSpan,
    #[serde(rename = "h")]
    pub content_hash: ContentHash,
    #[serde(rename = "c")]
    pub named_child_count: u32,
    #[serde(rename = "v", skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(rename = "a", skip_serializing_if = "Option::is_none")]
    pub locator_path: Option<Vec<AncestorStep>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditSyntaxInspection {
    #[serde(rename = "s")]
    pub scope: LocatorScope,
    #[serde(rename = "b")]
    pub base_ancestor_path: Vec<AncestorStep>,
    #[serde(rename = "n")]
    pub nodes: Vec<EditSyntaxNodeView>,
    #[serde(rename = "x")]
    pub truncated: bool,
    #[serde(rename = "t")]
    pub total_named_nodes_seen: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditSyntaxMutation {
    Replace(String),
    Delete,
    InsertBefore(String),
    InsertAfter(String),
}

#[derive(Debug, Clone)]
pub struct EditSyntaxPlanRequest {
    pub language_id: LanguageId,
    pub source: Arc<[u8]>,
    pub current_generation: Generation,
    pub file_context: FileContext,
    pub locator: NodeLocator,
    pub operation: EditSyntaxMutation,
    pub next_generation: Generation,
    pub inspection: EditInspectRequest,
}

#[derive(Debug, Clone)]
pub struct EditSyntaxPlan {
    pub source: Arc<[u8]>,
    pub old_file_hash: ContentHash,
    pub new_file_hash: ContentHash,
    pub changed_ranges: Vec<SourceSpan>,
    pub before_diagnostic_total: usize,
    pub before_diagnostics_truncated: bool,
    pub after_diagnostic_total: usize,
    pub after_diagnostics_truncated: bool,
    pub diagnostics: Vec<EditSyntaxDiagnostic>,
    pub inspection: EditSyntaxInspection,
    pub locators: Vec<NodeLocator>,
}

#[derive(Debug, Clone)]
pub struct EditSyntaxCreateRequest {
    pub language_id: LanguageId,
    pub source: Arc<[u8]>,
    pub generation: Generation,
    pub file_context: FileContext,
}

#[derive(Debug, Clone)]
pub struct EditSyntaxCreate {
    pub source: Arc<[u8]>,
    pub content_hash: ContentHash,
    pub diagnostic_total: usize,
    pub diagnostics_truncated: bool,
    pub diagnostics: Vec<EditSyntaxDiagnostic>,
    pub locators: Vec<NodeLocator>,
}

#[derive(Debug)]
pub enum EditSyntaxError {
    StaleLocator {
        cause: String,
        fresh: Box<EditSyntaxInspection>,
    },
    Adapter(PortError),
}

impl fmt::Display for EditSyntaxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleLocator { cause, .. } => write!(formatter, "node locator is stale: {cause}"),
            Self::Adapter(error) => error.fmt(formatter),
        }
    }
}

impl Error for EditSyntaxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::StaleLocator { .. } => None,
            Self::Adapter(error) => Some(error),
        }
    }
}

impl From<PortError> for EditSyntaxError {
    fn from(error: PortError) -> Self {
        Self::Adapter(error)
    }
}

/// Syntax mechanics required by structural edit use cases.
pub trait EditSyntax: Send + Sync {
    /// Plans one syntax-aware source mutation without writing files.
    ///
    /// # Errors
    ///
    /// Returns a stale-locator error with a fresh bounded inspection, or an adapter failure.
    fn plan_edit(&self, request: EditSyntaxPlanRequest) -> Result<EditSyntaxPlan, EditSyntaxError>;

    /// Parses and validates source for a new file.
    ///
    /// # Errors
    ///
    /// Returns an adapter failure when parsing or locator construction fails.
    fn parse_create(&self, request: EditSyntaxCreateRequest)
    -> Result<EditSyntaxCreate, PortError>;
}

fn serialize_source_span<S>(span: &SourceSpan, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    [
        span.bytes.start,
        span.bytes.end,
        span.start.row,
        span.start.column_bytes,
        span.end.row,
        span.end.column_bytes,
    ]
    .serialize(serializer)
}
