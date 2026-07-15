use goldeneye_domain::{
    AncestorStep, ByteSpan, ContentHash, FileContext, LocatorScope, NodeAnchor, NodeLocator,
    SourcePoint, SourceSpan, SyntaxIdentityError,
};
use thiserror::Error;
use tree_sitter::{Node, Point};

use crate::SyntaxSnapshot;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LocatorError {
    #[error("locator project does not match the current project")]
    ProjectMismatch,
    #[error("locator path does not match the current project-relative path")]
    PathMismatch,
    #[error("locator language does not match the syntax snapshot")]
    LanguageMismatch,
    #[error("locator grammar provider does not match the syntax snapshot")]
    GrammarProviderMismatch,
    #[error("locator grammar name does not match the syntax snapshot")]
    GrammarNameMismatch,
    #[error("locator grammar revision does not match the syntax snapshot")]
    GrammarRevisionMismatch,
    #[error("locator grammar ABI does not match the syntax snapshot")]
    GrammarAbiMismatch,
    #[error("locator file hash does not match the syntax snapshot")]
    FileHashMismatch,
    #[error("locator generation does not match the syntax snapshot")]
    GenerationMismatch,
    #[error("locator named-child index is out of bounds at ancestor depth {depth}")]
    AncestorIndexOutOfBounds { depth: usize },
    #[error("locator node kind does not match at ancestor depth {depth}")]
    AncestorKindMismatch { depth: usize },
    #[error("locator field name does not match at ancestor depth {depth}")]
    AncestorFieldMismatch { depth: usize },
    #[error("locator terminal node kind does not match")]
    TerminalKindMismatch,
    #[error("locator terminal byte range does not match")]
    TerminalByteRangeMismatch,
    #[error("locator terminal point span does not match")]
    TerminalPointSpanMismatch,
    #[error("locator terminal content hash does not match")]
    TerminalContentHashMismatch,
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
    #[error("cannot construct validated locator identity: {source}")]
    InvalidIdentity {
        #[source]
        source: SyntaxIdentityError,
    },
}

#[must_use]
pub fn locator_scope(snapshot: &SyntaxSnapshot, file_context: &FileContext) -> LocatorScope {
    LocatorScope::new(
        file_context.clone(),
        snapshot.language_id().clone(),
        snapshot.grammar().clone(),
        snapshot.file_hash(),
        snapshot.generation(),
    )
}

/// Builds an exact locator for every named node in deterministic preorder.
///
/// # Errors
///
/// Returns a typed error when a Tree-sitter coordinate or child index cannot be
/// represented by the portable domain model, or Tree-sitter reports an invalid
/// source range.
pub fn all_named_locators(
    snapshot: &SyntaxSnapshot,
    file_context: &FileContext,
) -> Result<Vec<NodeLocator>, LocatorError> {
    let scope = locator_scope(snapshot, file_context);
    let mut locators = Vec::new();
    let mut stack = vec![(snapshot.root(), Vec::<AncestorStep>::new())];

    while let Some((node, path)) = stack.pop() {
        if node.is_named() {
            locators.push(locator_for_node(snapshot, &scope, &path, node)?);
        }

        let children = named_children(node)?;
        for (child, step) in children.into_iter().rev() {
            let mut child_path = path.clone();
            child_path.push(step);
            stack.push((child, child_path));
        }
    }

    Ok(locators)
}

/// Resolves a locator only when every scope, ancestry, terminal, and content
/// guard exactly matches the immutable snapshot.
///
/// # Errors
///
/// Returns the first failed guard in the documented resolution order. This
/// function never performs byte-only or fuzzy relocation.
pub fn resolve_locator<'tree>(
    snapshot: &'tree SyntaxSnapshot,
    current_file_context: &FileContext,
    locator: &NodeLocator,
) -> Result<Node<'tree>, LocatorError> {
    let actual_scope = locator_scope(snapshot, current_file_context);
    validate_scope(&actual_scope, &locator.scope)?;

    let mut node = snapshot.root();
    for (depth, step) in locator.anchor.ancestor_path.iter().enumerate() {
        let (child, raw_child_index) = named_child_at(node, step.named_child_index)?
            .ok_or(LocatorError::AncestorIndexOutOfBounds { depth })?;
        if child.kind() != step.node_kind {
            return Err(LocatorError::AncestorKindMismatch { depth });
        }
        if node.field_name_for_child(raw_child_index) != step.field_name.as_deref() {
            return Err(LocatorError::AncestorFieldMismatch { depth });
        }
        node = child;
    }

    if node.kind() != locator.anchor.node_kind {
        return Err(LocatorError::TerminalKindMismatch);
    }

    let actual_span = node_span(node)?;
    if actual_span.bytes != locator.anchor.source_span.bytes {
        return Err(LocatorError::TerminalByteRangeMismatch);
    }
    if actual_span.start != locator.anchor.source_span.start
        || actual_span.end != locator.anchor.source_span.end
    {
        return Err(LocatorError::TerminalPointSpanMismatch);
    }

    let bytes = node_bytes(snapshot, node)?;
    if ContentHash::of(bytes) != locator.anchor.content_hash {
        return Err(LocatorError::TerminalContentHashMismatch);
    }

    Ok(node)
}

fn validate_scope(actual: &LocatorScope, claimed: &LocatorScope) -> Result<(), LocatorError> {
    if actual.file.project_id != claimed.file.project_id {
        return Err(LocatorError::ProjectMismatch);
    }
    if actual.file.relative_path != claimed.file.relative_path {
        return Err(LocatorError::PathMismatch);
    }
    if actual.language_id != claimed.language_id {
        return Err(LocatorError::LanguageMismatch);
    }
    if actual.grammar.provider != claimed.grammar.provider {
        return Err(LocatorError::GrammarProviderMismatch);
    }
    if actual.grammar.grammar != claimed.grammar.grammar {
        return Err(LocatorError::GrammarNameMismatch);
    }
    if actual.grammar.revision != claimed.grammar.revision {
        return Err(LocatorError::GrammarRevisionMismatch);
    }
    if actual.grammar.abi != claimed.grammar.abi {
        return Err(LocatorError::GrammarAbiMismatch);
    }
    if actual.file_hash != claimed.file_hash {
        return Err(LocatorError::FileHashMismatch);
    }
    if actual.generation != claimed.generation {
        return Err(LocatorError::GenerationMismatch);
    }
    Ok(())
}

fn locator_for_node(
    snapshot: &SyntaxSnapshot,
    scope: &LocatorScope,
    path: &[AncestorStep],
    node: Node<'_>,
) -> Result<NodeLocator, LocatorError> {
    let span = node_span(node)?;
    let content_hash = ContentHash::of(node_bytes(snapshot, node)?);
    let anchor = NodeAnchor::new(path.to_vec(), node.kind(), span, content_hash)
        .map_err(invalid_identity)?;
    Ok(NodeLocator::new(scope.clone(), anchor))
}

fn named_children(node: Node<'_>) -> Result<Vec<(Node<'_>, AncestorStep)>, LocatorError> {
    let mut children = Vec::with_capacity(node.named_child_count());
    let mut named_index = 0_u32;

    for raw_index in 0..node.child_count() {
        let raw_index =
            u32::try_from(raw_index).map_err(|_| LocatorError::TreeSitterChildIndexOverflow)?;
        let child = node
            .child(raw_index)
            .ok_or(LocatorError::TreeSitterChildUnavailable { raw_index })?;
        if !child.is_named() {
            continue;
        }

        let field_name = node.field_name_for_child(raw_index).map(str::to_owned);
        let step =
            AncestorStep::new(child.kind(), named_index, field_name).map_err(invalid_identity)?;
        children.push((child, step));
        named_index = named_index
            .checked_add(1)
            .ok_or(LocatorError::NamedChildIndexOverflow)?;
    }

    Ok(children)
}

fn named_child_at(
    node: Node<'_>,
    requested_named_index: u32,
) -> Result<Option<(Node<'_>, u32)>, LocatorError> {
    let mut named_index = 0_u32;

    for raw_index in 0..node.child_count() {
        let raw_index =
            u32::try_from(raw_index).map_err(|_| LocatorError::TreeSitterChildIndexOverflow)?;
        let child = node
            .child(raw_index)
            .ok_or(LocatorError::TreeSitterChildUnavailable { raw_index })?;
        if !child.is_named() {
            continue;
        }
        if named_index == requested_named_index {
            return Ok(Some((child, raw_index)));
        }
        named_index = named_index
            .checked_add(1)
            .ok_or(LocatorError::NamedChildIndexOverflow)?;
    }

    Ok(None)
}

fn node_span(node: Node<'_>) -> Result<SourceSpan, LocatorError> {
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

fn source_point(field: &'static str, point: Point) -> Result<SourcePoint, LocatorError> {
    Ok(SourcePoint::new(
        usize_to_u64(field, point.row)?,
        usize_to_u64(field, point.column)?,
    ))
}

fn usize_to_u64(field: &'static str, value: usize) -> Result<u64, LocatorError> {
    u64::try_from(value).map_err(|_| LocatorError::TreeSitterCoordinateOverflow { field })
}

fn node_bytes<'source>(
    snapshot: &'source SyntaxSnapshot,
    node: Node<'_>,
) -> Result<&'source [u8], LocatorError> {
    snapshot
        .source()
        .get(node.start_byte()..node.end_byte())
        .ok_or(LocatorError::NodeRangeOutOfBounds)
}

fn invalid_identity(source: SyntaxIdentityError) -> LocatorError {
    LocatorError::InvalidIdentity { source }
}
