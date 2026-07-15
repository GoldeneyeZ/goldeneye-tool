//! Tool-neutral structural edit requests and compact results.

mod inspection;
mod results;
mod runtime;

use goldeneye_domain::{ContentHash, SourceSpan};
use goldeneye_edit::ParsePolicy;
use serde::{Deserialize, Serialize};

use crate::{Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath};

const BYTES_PER_APPROXIMATE_TOKEN: usize = 4;

/// Parse validation policy shared by create and structural edit tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditParsePolicy {
    RequireClean,
    NoAdditionalDiagnostics,
    AllowErrors,
}

impl From<EditParsePolicy> for ParsePolicy {
    fn from(value: EditParsePolicy) -> Self {
        match value {
            EditParsePolicy::RequireClean => Self::RequireClean,
            EditParsePolicy::NoAdditionalDiagnostics => Self::NoAdditionalDiagnostics,
            EditParsePolicy::AllowErrors => Self::AllowErrors,
        }
    }
}

const fn default_edit_parse_policy() -> EditParsePolicy {
    EditParsePolicy::NoAdditionalDiagnostics
}

const fn default_create_parse_policy() -> EditParsePolicy {
    EditParsePolicy::RequireClean
}

/// Compact syntax inspection for one indexed project-relative file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectSyntaxRequest {
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    #[serde(default)]
    pub inspect: goldeneye_ports::InspectRequest,
}

impl InspectSyntaxRequest {
    #[must_use]
    pub fn new(project: ProjectId, path: ProjectRelativePath) -> Self {
        Self {
            project,
            path,
            inspect: goldeneye_ports::InspectRequest::default(),
        }
    }
}

/// Content-bearing request shared by replace and adjacent insert operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeContentRequest {
    pub operation_id: String,
    pub locator: NodeLocator,
    pub content: String,
    #[serde(default = "default_edit_parse_policy")]
    pub parse_policy: EditParsePolicy,
}

impl NodeContentRequest {
    #[must_use]
    pub fn new(
        operation_id: impl Into<String>,
        locator: NodeLocator,
        content: impl Into<String>,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            locator,
            content: content.into(),
            parse_policy: default_edit_parse_policy(),
        }
    }
}

/// Exact named-node deletion request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteNodeRequest {
    pub operation_id: String,
    pub locator: NodeLocator,
    #[serde(default = "default_edit_parse_policy")]
    pub parse_policy: EditParsePolicy,
}

impl DeleteNodeRequest {
    #[must_use]
    pub fn new(operation_id: impl Into<String>, locator: NodeLocator) -> Self {
        Self {
            operation_id: operation_id.into(),
            locator,
            parse_policy: default_edit_parse_policy(),
        }
    }
}

/// No-overwrite project-relative file creation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFileRequest {
    pub operation_id: String,
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    pub content: String,
    pub expected_generation: Generation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language_id: Option<LanguageId>,
    #[serde(default = "default_create_parse_policy")]
    pub parse_policy: EditParsePolicy,
    #[serde(default)]
    pub create_parents: bool,
}

impl CreateFileRequest {
    #[must_use]
    pub fn new(
        operation_id: impl Into<String>,
        project: ProjectId,
        path: ProjectRelativePath,
        content: impl Into<String>,
        expected_generation: Generation,
    ) -> Self {
        Self {
            operation_id: operation_id.into(),
            project,
            path,
            content: content.into(),
            expected_generation,
            language_id: None,
            parse_policy: default_create_parse_policy(),
            create_parents: false,
        }
    }

    #[must_use]
    pub const fn with_parent_creation(mut self, create_parents: bool) -> Self {
        self.create_parents = create_parents;
        self
    }
}

/// JSON-safe diagnostic returned without Tree-sitter implementation details.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxDiagnosticResult {
    pub kind: String,
    pub node_kind: String,
    pub span: SourceSpan,
}

/// Bounded parse diagnostic summary before and after a mutation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationDiagnostics {
    pub before_total: usize,
    pub after_total: usize,
    pub before_truncated: bool,
    pub after_truncated: bool,
    pub after: Vec<SyntaxDiagnosticResult>,
}

/// Minimal byte-span diff. Changed content is represented by size and hashes, not echoed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationDiff {
    pub old_span: goldeneye_domain::ByteSpan,
    pub new_span: goldeneye_domain::ByteSpan,
    pub removed_hash: ContentHash,
    pub inserted_hash: ContentHash,
    pub inserted_bytes: usize,
}

/// Compact graph delta and bounded changed stable IDs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphMutation {
    pub added: usize,
    pub removed: usize,
    pub retained: usize,
}

/// Token-oriented mutation result sizing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MutationSize {
    pub source_bytes: usize,
    pub changed_bytes: usize,
    pub compact_syntax_bytes: usize,
    pub refreshed_locator_bytes: usize,
    pub approximate_context_tokens: usize,
}

/// Durable mutation output shared by direct services and MCP.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditMutationResult {
    pub operation_id: String,
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    pub old_file_hash: Option<ContentHash>,
    pub new_file_hash: ContentHash,
    pub diff: MutationDiff,
    pub changed_syntax_ids: Vec<NodeLocator>,
    pub changed_graph_ids: Vec<String>,
    pub graph: GraphMutation,
    pub generation: Generation,
    pub diagnostics: MutationDiagnostics,
    pub size: MutationSize,
}

/// Token-oriented inspection size metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectionSize {
    pub source_bytes: usize,
    pub compact_syntax_bytes: usize,
    pub locator_bytes: usize,
    pub approximate_context_tokens: usize,
}

/// Compact syntax plus fully serializable locators in matching node order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InspectSyntaxResult {
    pub project: ProjectId,
    pub path: ProjectRelativePath,
    pub language_id: LanguageId,
    pub file_hash: ContentHash,
    pub generation: Generation,
    pub syntax: goldeneye_ports::SyntaxInspection,
    pub locators: Vec<NodeLocator>,
    pub diagnostic_total: usize,
    pub diagnostics_truncated: bool,
    pub diagnostics: Vec<SyntaxDiagnosticResult>,
    pub size: InspectionSize,
}
