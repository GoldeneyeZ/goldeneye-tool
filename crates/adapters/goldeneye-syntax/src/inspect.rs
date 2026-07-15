use goldeneye_domain::{
    AncestorStep, ByteSpan, ContentHash, FileContext, SourcePoint, SourceSpan, SyntaxIdentityError,
};
pub use goldeneye_ports::{
    DEFAULT_MAX_DEPTH, DEFAULT_MAX_NODES, DEFAULT_PREVIEW_CHARS, InspectError, InspectRequest,
    MAX_INSPECT_DEPTH, MAX_INSPECT_KIND_FILTERS, MAX_INSPECT_NODES, MAX_PREVIEW_CHARS,
    SyntaxInspection, SyntaxNodeView,
};
use tree_sitter::{Node, Point};

use crate::{SyntaxSnapshot, locator_scope};

#[derive(Clone)]
struct TraversalItem<'tree> {
    node: Node<'tree>,
    parent_ordinal: Option<u32>,
    depth: usize,
    relative_step: Option<AncestorStep>,
    locator_path: Vec<AncestorStep>,
}

/// Inspects named syntax nodes using bounded iterative preorder traversal.
///
/// # Errors
///
/// Returns typed request validation, Tree-sitter invariant, coordinate, and
/// identity errors. Limits above hard caps are rejected rather than clamped.
#[allow(clippy::too_many_lines)]
pub fn inspect_syntax(
    snapshot: &SyntaxSnapshot,
    file_context: &FileContext,
    request: &InspectRequest,
) -> Result<SyntaxInspection, InspectError> {
    validate_request(snapshot, request)?;
    let range = request.byte_range.as_ref();
    let (base, base_ancestor_path) = select_base(snapshot, range)?;
    let filtering = !request.node_kinds.is_empty();
    let mut stack = vec![TraversalItem {
        node: base,
        parent_ordinal: None,
        depth: 0,
        relative_step: None,
        locator_path: Vec::new(),
    }];
    let mut nodes = Vec::with_capacity(request.max_nodes.min(DEFAULT_MAX_NODES));
    let mut total_named_nodes_seen = 0_usize;
    let mut depth_truncated = false;

    while let Some(item) = stack.pop() {
        let span = node_span(item.node)?;
        if !span_is_relevant(span.bytes, range) {
            continue;
        }

        let children = named_children(item.node)?;
        let named_child_count =
            u32::try_from(children.len()).map_err(|_| InspectError::CountOverflow {
                field: "named_child_count",
            })?;

        let matches_kind = !filtering
            || request
                .node_kinds
                .iter()
                .any(|kind| kind == item.node.kind());
        if matches_kind {
            total_named_nodes_seen =
                total_named_nodes_seen
                    .checked_add(1)
                    .ok_or(InspectError::CountOverflow {
                        field: "total_named_nodes_seen",
                    })?;
        }

        let emitted_ordinal = if matches_kind && nodes.len() < request.max_nodes {
            let ordinal = u32::try_from(nodes.len())
                .map_err(|_| InspectError::CountOverflow { field: "ordinal" })?;
            let bytes = node_bytes(snapshot, item.node)?;
            nodes.push(SyntaxNodeView {
                ordinal,
                parent_ordinal: if filtering { None } else { item.parent_ordinal },
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
                locator_path: filtering.then_some(item.locator_path.clone()),
            });
            Some(ordinal)
        } else {
            None
        };

        if item.depth < request.max_depth {
            for (child, step) in children.into_iter().rev() {
                if span_is_relevant(node_span(child)?.bytes, range) {
                    let mut locator_path = if filtering {
                        item.locator_path.clone()
                    } else {
                        Vec::new()
                    };
                    if filtering {
                        locator_path.push(step.clone());
                    }
                    stack.push(TraversalItem {
                        node: child,
                        parent_ordinal: emitted_ordinal,
                        depth: item.depth + 1,
                        relative_step: Some(step),
                        locator_path,
                    });
                }
            }
        } else if children.iter().any(|(child, _)| {
            node_span(*child).is_ok_and(|span| span_is_relevant(span.bytes, range))
        }) {
            depth_truncated = true;
        }
    }

    let truncated = depth_truncated || total_named_nodes_seen > nodes.len();
    let inspection = SyntaxInspection {
        scope: locator_scope(snapshot, file_context),
        base_ancestor_path,
        nodes,
        truncated,
        total_named_nodes_seen,
    };
    inspection.validate()?;
    Ok(inspection)
}

fn validate_request(
    snapshot: &SyntaxSnapshot,
    request: &InspectRequest,
) -> Result<(), InspectError> {
    validate_limit("max_depth", request.max_depth, MAX_INSPECT_DEPTH)?;
    validate_limit("max_nodes", request.max_nodes, MAX_INSPECT_NODES)?;
    validate_limit("preview_chars", request.preview_chars, MAX_PREVIEW_CHARS)?;
    validate_limit(
        "node_kinds",
        request.node_kinds.len(),
        MAX_INSPECT_KIND_FILTERS,
    )?;

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

fn invalid_identity(source: SyntaxIdentityError) -> InspectError {
    InspectError::InvalidIdentity { source }
}
