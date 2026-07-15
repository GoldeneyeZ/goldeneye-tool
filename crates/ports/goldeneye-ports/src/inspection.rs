use goldeneye_domain::{
    AncestorStep, ByteSpan, ContentHash, LocatorScope, NodeAnchor, NodeLocator, SourceSpan,
    SyntaxIdentityError,
};
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

pub const DEFAULT_MAX_DEPTH: usize = 4;
pub const DEFAULT_MAX_NODES: usize = 200;
pub const DEFAULT_PREVIEW_CHARS: usize = 0;
pub const MAX_INSPECT_DEPTH: usize = 32;
pub const MAX_INSPECT_NODES: usize = 1_000;
pub const MAX_PREVIEW_CHARS: usize = 256;
pub const MAX_INSPECT_KIND_FILTERS: usize = 32;

/// Bounds one syntax inspection. Depth is relative to the selected base node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectRequest {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub preview_chars: usize,
    pub byte_range: Option<ByteSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_kinds: Vec<String>,
}

impl Default for InspectRequest {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_nodes: DEFAULT_MAX_NODES,
            preview_chars: DEFAULT_PREVIEW_CHARS,
            byte_range: None,
            node_kinds: Vec::new(),
        }
    }
}

/// One named syntax node in compact preorder form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxNodeView {
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

/// Bounded inspection wire model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyntaxInspection {
    #[serde(rename = "s")]
    pub scope: LocatorScope,
    #[serde(rename = "b")]
    pub base_ancestor_path: Vec<AncestorStep>,
    #[serde(rename = "n")]
    pub nodes: Vec<SyntaxNodeView>,
    #[serde(rename = "x")]
    pub truncated: bool,
    #[serde(rename = "t")]
    pub total_named_nodes_seen: usize,
}

impl SyntaxInspection {
    /// Validates compact parent-delta invariants.
    ///
    /// # Errors
    ///
    /// Returns an invalid-parent error when the compact model is malformed.
    pub fn validate(&self) -> Result<(), InspectError> {
        validate_parent_links(&self.nodes)
    }

    /// Reconstructs the exact guarded locator represented by an emitted node.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an unknown ordinal or malformed parent delta.
    pub fn locator(&self, ordinal: u32) -> Result<NodeLocator, InspectError> {
        self.validate()?;
        let target_index =
            usize::try_from(ordinal).map_err(|_| InspectError::UnknownOrdinal { ordinal })?;
        let target = self
            .nodes
            .get(target_index)
            .filter(|node| node.ordinal == ordinal)
            .ok_or(InspectError::UnknownOrdinal { ordinal })?;
        if let Some(locator_path) = &target.locator_path {
            let mut ancestor_path = self.base_ancestor_path.clone();
            ancestor_path.extend(locator_path.iter().cloned());
            return locator(&self.scope, ancestor_path, target);
        }
        let mut suffix = Vec::with_capacity(target.depth);
        let mut cursor_index = target_index;
        while let Some(parent_ordinal) = self.nodes[cursor_index].parent_ordinal {
            let node = &self.nodes[cursor_index];
            let named_child_index =
                node.named_child_index
                    .ok_or(InspectError::InvalidParentLink {
                        ordinal: node.ordinal,
                    })?;
            suffix.push(
                AncestorStep::new(
                    node.kind.clone(),
                    named_child_index,
                    node.field_name.clone(),
                )
                .map_err(invalid_identity)?,
            );
            cursor_index =
                usize::try_from(parent_ordinal).map_err(|_| InspectError::InvalidParentLink {
                    ordinal: node.ordinal,
                })?;
        }
        suffix.reverse();
        let mut ancestor_path = self.base_ancestor_path.clone();
        ancestor_path.extend(suffix);
        locator(&self.scope, ancestor_path, target)
    }
}

fn locator(
    scope: &LocatorScope,
    ancestor_path: Vec<AncestorStep>,
    target: &SyntaxNodeView,
) -> Result<NodeLocator, InspectError> {
    let anchor = NodeAnchor::new(
        ancestor_path,
        target.kind.clone(),
        target.span,
        target.content_hash,
    )
    .map_err(invalid_identity)?;
    Ok(NodeLocator::new(scope.clone(), anchor))
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum InspectError {
    #[error("inspection limit {field}={requested} exceeds hard cap {maximum}")]
    LimitExceeded {
        field: &'static str,
        requested: usize,
        maximum: usize,
    },
    #[error("inspection byte range start {start} exceeds end {end}")]
    InvalidRange { start: u64, end: u64 },
    #[error("inspection byte range {start}..{end} lies outside source length {source_len}")]
    RangeOutOfBounds {
        start: u64,
        end: u64,
        source_len: u64,
    },
    #[error("inspection has no node with ordinal {ordinal}")]
    UnknownOrdinal { ordinal: u32 },
    #[error("inspection node {ordinal} has an invalid parent delta")]
    InvalidParentLink { ordinal: u32 },
    #[error("Tree-sitter child index cannot be represented as u32")]
    TreeSitterChildIndexOverflow,
    #[error("Tree-sitter named-child index cannot be represented as u32")]
    NamedChildIndexOverflow,
    #[error("Tree-sitter coordinate {field} cannot be represented as u64")]
    TreeSitterCoordinateOverflow { field: &'static str },
    #[error("Tree-sitter did not return child {raw_index} from its declared child range")]
    TreeSitterChildUnavailable { raw_index: u32 },
    #[error("Tree-sitter node range lies outside the immutable snapshot source")]
    NodeRangeOutOfBounds,
    #[error("inspection count {field} overflowed its portable representation")]
    CountOverflow { field: &'static str },
    #[error("cannot construct validated inspection identity: {source}")]
    InvalidIdentity {
        #[source]
        source: SyntaxIdentityError,
    },
}

fn validate_parent_links(nodes: &[SyntaxNodeView]) -> Result<(), InspectError> {
    for (index, node) in nodes.iter().enumerate() {
        let ordinal =
            u32::try_from(index).map_err(|_| InspectError::CountOverflow { field: "ordinal" })?;
        if node.ordinal != ordinal {
            return Err(InspectError::InvalidParentLink {
                ordinal: node.ordinal,
            });
        }
        if let Some(path) = &node.locator_path {
            let valid = node.parent_ordinal.is_none()
                && node.depth == path.len()
                && match path.last() {
                    Some(step) => {
                        node.named_child_index == Some(step.named_child_index)
                            && node.field_name == step.field_name
                            && node.kind == step.node_kind
                    }
                    None => node.named_child_index.is_none() && node.field_name.is_none(),
                };
            if !valid {
                return Err(InspectError::InvalidParentLink {
                    ordinal: node.ordinal,
                });
            }
            continue;
        }
        match node.parent_ordinal {
            None if index == 0 && node.depth == 0 && node.named_child_index.is_none() => {}
            Some(parent_ordinal) if parent_ordinal < node.ordinal => {
                let parent = nodes
                    .get(usize::try_from(parent_ordinal).map_err(|_| {
                        InspectError::InvalidParentLink {
                            ordinal: node.ordinal,
                        }
                    })?)
                    .ok_or(InspectError::InvalidParentLink {
                        ordinal: node.ordinal,
                    })?;
                if parent.depth.checked_add(1) != Some(node.depth)
                    || node.named_child_index.is_none()
                {
                    return Err(InspectError::InvalidParentLink {
                        ordinal: node.ordinal,
                    });
                }
            }
            _ => {
                return Err(InspectError::InvalidParentLink {
                    ordinal: node.ordinal,
                });
            }
        }
    }
    Ok(())
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

fn invalid_identity(source: SyntaxIdentityError) -> InspectError {
    InspectError::InvalidIdentity { source }
}
