use goldeneye_domain::{
    AncestorStep, ByteSpan, ContentHash, FileContext, LocatorScope, NodeAnchor, NodeLocator,
    SourcePoint, SourceSpan, SyntaxIdentityError,
};
use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;
use tree_sitter::{Node, Point};

use crate::{SyntaxSnapshot, locator_scope};

pub const DEFAULT_MAX_DEPTH: usize = 4;
pub const DEFAULT_MAX_NODES: usize = 200;
pub const DEFAULT_PREVIEW_CHARS: usize = 0;
pub const MAX_INSPECT_DEPTH: usize = 32;
pub const MAX_INSPECT_NODES: usize = 1_000;
pub const MAX_PREVIEW_CHARS: usize = 256;

/// Bounds one syntax inspection. Depth is relative to the selected base node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectRequest {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub preview_chars: usize,
    pub byte_range: Option<ByteSpan>,
}

impl Default for InspectRequest {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
            max_nodes: DEFAULT_MAX_NODES,
            preview_chars: DEFAULT_PREVIEW_CHARS,
            byte_range: None,
        }
    }
}

/// One named syntax node in compact preorder form.
///
/// Wire keys are `o` (ordinal), `p` (parent), `d` (depth), `i` (named-child
/// index), `f` (field), `k` (kind), `s` (six-coordinate span array), `h`
/// (content hash), `c` (named-child count), and `v` (preview).
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
}

/// Bounded inspection wire model.
///
/// `s` is the one shared locator scope, `b` is the one ranged base path, `n`
/// contains node deltas, `x` reports truncation, and `t` is the number of
/// relevant named nodes seen within the requested depth.
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
    /// Reconstructs the exact guarded locator represented by an emitted node.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an unknown ordinal or malformed parent delta.
    pub fn locator(&self, ordinal: u32) -> Result<NodeLocator, InspectError> {
        validate_parent_links(&self.nodes)?;
        let target_index =
            usize::try_from(ordinal).map_err(|_| InspectError::UnknownOrdinal { ordinal })?;
        let target = self
            .nodes
            .get(target_index)
            .filter(|node| node.ordinal == ordinal)
            .ok_or(InspectError::UnknownOrdinal { ordinal })?;

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
        let anchor = NodeAnchor::new(
            ancestor_path,
            target.kind.clone(),
            target.span,
            target.content_hash,
        )
        .map_err(invalid_identity)?;
        Ok(NodeLocator::new(self.scope.clone(), anchor))
    }
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

#[derive(Clone)]
struct TraversalItem<'tree> {
    node: Node<'tree>,
    parent_ordinal: Option<u32>,
    depth: usize,
    relative_step: Option<AncestorStep>,
}

/// Inspects named syntax nodes using bounded iterative preorder traversal.
///
/// # Errors
///
/// Returns typed request validation, Tree-sitter invariant, coordinate, and
/// identity errors. Limits above hard caps are rejected rather than clamped.
pub fn inspect_syntax(
    snapshot: &SyntaxSnapshot,
    file_context: &FileContext,
    request: &InspectRequest,
) -> Result<SyntaxInspection, InspectError> {
    validate_request(snapshot, request)?;
    let range = request.byte_range.as_ref();
    let (base, base_ancestor_path) = select_base(snapshot, range)?;
    let mut stack = vec![TraversalItem {
        node: base,
        parent_ordinal: None,
        depth: 0,
        relative_step: None,
    }];
    let mut nodes = Vec::with_capacity(request.max_nodes.min(DEFAULT_MAX_NODES));
    let mut total_named_nodes_seen = 0_usize;
    let mut depth_truncated = false;

    while let Some(item) = stack.pop() {
        let span = node_span(item.node)?;
        if !span_is_relevant(span.bytes, range) {
            continue;
        }

        total_named_nodes_seen =
            total_named_nodes_seen
                .checked_add(1)
                .ok_or(InspectError::CountOverflow {
                    field: "total_named_nodes_seen",
                })?;
        let children = named_children(item.node)?;
        let named_child_count =
            u32::try_from(children.len()).map_err(|_| InspectError::CountOverflow {
                field: "named_child_count",
            })?;

        let emitted_ordinal = if nodes.len() < request.max_nodes {
            let ordinal = u32::try_from(nodes.len())
                .map_err(|_| InspectError::CountOverflow { field: "ordinal" })?;
            let bytes = node_bytes(snapshot, item.node)?;
            nodes.push(SyntaxNodeView {
                ordinal,
                parent_ordinal: item.parent_ordinal,
                depth: item.depth,
                named_child_index: item
                    .relative_step
                    .as_ref()
                    .map(|step| step.named_child_index),
                field_name: item
                    .relative_step
                    .as_ref()
                    .and_then(|step| step.field_name.clone()),
                kind: item.node.kind().to_owned(),
                span,
                content_hash: ContentHash::of(bytes),
                named_child_count,
                preview: (request.preview_chars > 0)
                    .then(|| bounded_preview(bytes, request.preview_chars)),
            });
            Some(ordinal)
        } else {
            None
        };

        if item.depth < request.max_depth {
            for (child, step) in children.into_iter().rev() {
                if span_is_relevant(node_span(child)?.bytes, range) {
                    stack.push(TraversalItem {
                        node: child,
                        parent_ordinal: emitted_ordinal,
                        depth: item.depth + 1,
                        relative_step: Some(step),
                    });
                }
            }
        } else if children.iter().any(|(child, _)| {
            node_span(*child).is_ok_and(|span| span_is_relevant(span.bytes, range))
        }) {
            depth_truncated = true;
        }
    }

    validate_parent_links(&nodes)?;
    let truncated = depth_truncated || total_named_nodes_seen > nodes.len();
    Ok(SyntaxInspection {
        scope: locator_scope(snapshot, file_context),
        base_ancestor_path,
        nodes,
        truncated,
        total_named_nodes_seen,
    })
}

fn validate_request(
    snapshot: &SyntaxSnapshot,
    request: &InspectRequest,
) -> Result<(), InspectError> {
    validate_limit("max_depth", request.max_depth, MAX_INSPECT_DEPTH)?;
    validate_limit("max_nodes", request.max_nodes, MAX_INSPECT_NODES)?;
    validate_limit("preview_chars", request.preview_chars, MAX_PREVIEW_CHARS)?;

    if let Some(range) = request.byte_range {
        if range.start > range.end {
            return Err(InspectError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }
        let source_len =
            u64::try_from(snapshot.source().len()).map_err(|_| InspectError::CountOverflow {
                field: "source_len",
            })?;
        if range.start > source_len || range.end > source_len {
            return Err(InspectError::RangeOutOfBounds {
                start: range.start,
                end: range.end,
                source_len,
            });
        }
    }
    Ok(())
}

fn validate_limit(
    field: &'static str,
    requested: usize,
    maximum: usize,
) -> Result<(), InspectError> {
    if requested > maximum {
        return Err(InspectError::LimitExceeded {
            field,
            requested,
            maximum,
        });
    }
    Ok(())
}

fn select_base<'tree>(
    snapshot: &'tree SyntaxSnapshot,
    range: Option<&ByteSpan>,
) -> Result<(Node<'tree>, Vec<AncestorStep>), InspectError> {
    let mut node = snapshot.root();
    let mut path = Vec::new();
    let Some(range) = range else {
        return Ok((node, path));
    };

    loop {
        let mut containing_child = None;
        for (child, step) in named_children(node)? {
            if span_contains(node_span(child)?.bytes, range) {
                containing_child = Some((child, step));
                break;
            }
        }
        let Some((child, step)) = containing_child else {
            break;
        };
        node = child;
        path.push(step);
    }
    Ok((node, path))
}

fn span_contains(span: ByteSpan, range: &ByteSpan) -> bool {
    if range.start == range.end {
        span.start <= range.start && range.start < span.end
    } else {
        span.start <= range.start && range.end <= span.end
    }
}

fn span_is_relevant(span: ByteSpan, range: Option<&ByteSpan>) -> bool {
    let Some(range) = range else {
        return true;
    };
    if range.start == range.end {
        span.start <= range.start && range.start < span.end
    } else {
        span.start < range.end && range.start < span.end
    }
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
        match node.parent_ordinal {
            None if index == 0 && node.depth == 0 && node.named_child_index.is_none() => {}
            Some(parent_ordinal) if parent_ordinal < node.ordinal => {
                let parent_index = usize::try_from(parent_ordinal).map_err(|_| {
                    InspectError::InvalidParentLink {
                        ordinal: node.ordinal,
                    }
                })?;
                let parent = nodes
                    .get(parent_index)
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

fn named_children(node: Node<'_>) -> Result<Vec<(Node<'_>, AncestorStep)>, InspectError> {
    let mut children = Vec::with_capacity(node.named_child_count());
    let mut named_index = 0_u32;
    for raw_index in 0..node.child_count() {
        let raw_index =
            u32::try_from(raw_index).map_err(|_| InspectError::TreeSitterChildIndexOverflow)?;
        let child = node
            .child(raw_index)
            .ok_or(InspectError::TreeSitterChildUnavailable { raw_index })?;
        if !child.is_named() {
            continue;
        }
        children.push((
            child,
            AncestorStep::new(
                child.kind(),
                named_index,
                node.field_name_for_child(raw_index).map(str::to_owned),
            )
            .map_err(invalid_identity)?,
        ));
        named_index = named_index
            .checked_add(1)
            .ok_or(InspectError::NamedChildIndexOverflow)?;
    }
    Ok(children)
}

fn node_span(node: Node<'_>) -> Result<SourceSpan, InspectError> {
    let bytes = ByteSpan::new(
        usize_to_u64("node.start_byte", node.start_byte())?,
        usize_to_u64("node.end_byte", node.end_byte())?,
    )
    .map_err(invalid_identity)?;
    SourceSpan::new(
        bytes,
        source_point("node.start_position", node.start_position())?,
        source_point("node.end_position", node.end_position())?,
    )
    .map_err(invalid_identity)
}

fn source_point(field: &'static str, point: Point) -> Result<SourcePoint, InspectError> {
    Ok(SourcePoint::new(
        usize_to_u64(field, point.row)?,
        usize_to_u64(field, point.column)?,
    ))
}

fn usize_to_u64(field: &'static str, value: usize) -> Result<u64, InspectError> {
    u64::try_from(value).map_err(|_| InspectError::TreeSitterCoordinateOverflow { field })
}

fn node_bytes<'source>(
    snapshot: &'source SyntaxSnapshot,
    node: Node<'_>,
) -> Result<&'source [u8], InspectError> {
    snapshot
        .source()
        .get(node.start_byte()..node.end_byte())
        .ok_or(InspectError::NodeRangeOutOfBounds)
}

fn bounded_preview(bytes: &[u8], max_chars: usize) -> String {
    let decoded = String::from_utf8_lossy(bytes);
    let mut result = String::new();
    let mut used = 0_usize;
    for character in decoded.chars() {
        let atom = escape_atom(character);
        let atom_chars = atom.chars().count();
        if used + atom_chars > max_chars {
            break;
        }
        result.push_str(&atom);
        used += atom_chars;
    }
    result
}

fn escape_atom(character: char) -> String {
    match character {
        '\\' => "\\\\".to_owned(),
        '\n' => "\\n".to_owned(),
        '\r' => "\\r".to_owned(),
        '\t' => "\\t".to_owned(),
        '\u{2028}' | '\u{2029}' => format!("\\u{{{:x}}}", character as u32),
        control if control.is_control() => format!("\\u{{{:x}}}", control as u32),
        printable => printable.to_string(),
    }
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
