#![forbid(unsafe_code)]

use std::sync::Arc;

use goldeneye_domain::{
    ByteSpan, ContentHash, FileContext, Generation, LanguageId, NodeLocator, SourcePoint,
    SyntaxIdentityError,
};
use goldeneye_syntax::{
    GrammarProvider, InspectError, InspectRequest, LocatorError, SyntaxDiagnostic, SyntaxEdit,
    SyntaxEngine, SyntaxError, SyntaxInspection, SyntaxSnapshot, inspect_syntax, resolve_locator,
};
use thiserror::Error;

const DEFAULT_REFRESH_NODES: usize = 64;
const BYTES_PER_APPROXIMATE_TOKEN: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOperation {
    Replace(String),
    Delete,
    InsertBefore(String),
    InsertAfter(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParsePolicy {
    RequireClean,
    NoAdditionalDiagnostics,
    AllowErrors,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditOptions {
    pub parse_policy: ParsePolicy,
    pub refresh_request: InspectRequest,
}

impl Default for EditOptions {
    fn default() -> Self {
        Self {
            parse_policy: ParsePolicy::RequireClean,
            refresh_request: InspectRequest {
                max_nodes: DEFAULT_REFRESH_NODES,
                ..InspectRequest::default()
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDiagnostics {
    pub before_total: usize,
    pub after_total: usize,
    pub before_truncated: bool,
    pub after_truncated: bool,
    pub after: Vec<SyntaxDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDiff {
    pub old_span: ByteSpan,
    pub new_span: ByteSpan,
    pub removed_hash: ContentHash,
    pub inserted_hash: ContentHash,
    pub inserted: Arc<[u8]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenSizeMetadata {
    pub source_bytes: usize,
    pub changed_bytes: usize,
    pub compact_syntax_bytes: usize,
    pub refreshed_locator_bytes: usize,
    pub approximate_context_tokens: usize,
}

pub struct EditPlan {
    pub source: Arc<[u8]>,
    pub snapshot: SyntaxSnapshot,
    pub diff: SourceDiff,
    pub changed_ranges: Vec<goldeneye_domain::SourceSpan>,
    pub old_file_hash: ContentHash,
    pub new_file_hash: ContentHash,
    pub diagnostics: EditDiagnostics,
    pub refreshed_syntax: SyntaxInspection,
    pub refreshed_locators: Vec<NodeLocator>,
    pub token_size: TokenSizeMetadata,
}

pub struct ValidatedCreate {
    pub source: Arc<[u8]>,
    pub snapshot: SyntaxSnapshot,
    pub content_hash: ContentHash,
    pub diagnostics: EditDiagnostics,
    pub token_size: TokenSizeMetadata,
}

#[derive(Debug, Error)]
pub enum EditError {
    #[error("node locator is stale: {cause}")]
    StaleLocator {
        cause: LocatorError,
        fresh: Box<SyntaxInspection>,
    },
    #[error(
        "proposed source rejected by {policy:?}: {after_total} diagnostics after {before_total}"
    )]
    ParseRejected {
        policy: ParsePolicy,
        before_total: usize,
        after_total: usize,
        proposed_file_hash: ContentHash,
        diagnostics: Vec<SyntaxDiagnostic>,
    },
    #[error("source size arithmetic overflowed")]
    SourceSizeOverflow,
    #[error("source offset cannot be represented by the portable byte model")]
    SourceOffsetOverflow,
    #[error("cannot construct edit identity: {source}")]
    InvalidIdentity {
        #[source]
        source: SyntaxIdentityError,
    },
    #[error("cannot encode compact edit metadata: {source}")]
    MetadataEncoding {
        #[source]
        source: serde_json::Error,
    },
    #[error(transparent)]
    Inspect(#[from] InspectError),
    #[error(transparent)]
    Syntax(#[from] SyntaxError),
}

/// Plans one exact named-node mutation without writing to the filesystem.
///
/// # Errors
///
/// Returns a typed stale-locator error with fresh syntax context when any
/// identity guard fails. Syntax, inspection, parse-policy, size, identity, and
/// metadata failures are returned without mutating `snapshot`.
pub fn plan_edit<P>(
    engine: &SyntaxEngine<P>,
    snapshot: &SyntaxSnapshot,
    file_context: &FileContext,
    locator: &NodeLocator,
    operation: &EditOperation,
    next_generation: Generation,
    options: &EditOptions,
) -> Result<EditPlan, EditError>
where
    P: GrammarProvider,
{
    let node = match resolve_locator(snapshot, file_context, locator) {
        Ok(node) => node,
        Err(cause) => {
            let fresh = inspect_syntax(
                snapshot,
                file_context,
                &stale_view_request(snapshot, locator),
            )?;
            return Err(EditError::StaleLocator {
                cause,
                fresh: Box::new(fresh),
            });
        }
    };
    let node_start = node.start_byte();
    let node_end = node.end_byte();
    let (start, old_end, replacement) = operation_geometry(operation, node_start, node_end);
    let proposed = splice(snapshot.source(), start, old_end, replacement)?;
    let new_end = start
        .checked_add(replacement.len())
        .ok_or(EditError::SourceSizeOverflow)?;
    let syntax_edit = SyntaxEdit::new(
        portable_offset(start)?,
        portable_offset(old_end)?,
        portable_offset(new_end)?,
        point_at(snapshot.source(), start),
        point_at(snapshot.source(), old_end),
        point_at(&proposed, new_end),
    );
    let source = Arc::<[u8]>::from(proposed);
    let reparsed = engine.reparse(
        snapshot,
        snapshot.language_id().clone(),
        Arc::clone(&source),
        next_generation,
        syntax_edit,
    )?;
    let diagnostics = diagnostics(snapshot, &reparsed.snapshot);
    enforce_parse_policy(
        options.parse_policy,
        &diagnostics,
        reparsed.snapshot.file_hash(),
    )?;

    let diff = minimal_diff(snapshot.source(), &source)?;
    let mut refresh_request = options.refresh_request.clone();
    refresh_request.byte_range = Some(diff.new_span);
    let refreshed_syntax = inspect_syntax(&reparsed.snapshot, file_context, &refresh_request)?;
    let refreshed_locators = refreshed_syntax
        .nodes
        .iter()
        .map(|node| refreshed_syntax.locator(node.ordinal))
        .collect::<Result<Vec<_>, _>>()?;
    let token_size = token_size(
        source.len(),
        diff.inserted.len(),
        &refreshed_syntax,
        &refreshed_locators,
    )?;
    let old_file_hash = snapshot.file_hash();
    let new_file_hash = reparsed.snapshot.file_hash();

    Ok(EditPlan {
        source,
        snapshot: reparsed.snapshot,
        diff,
        changed_ranges: reparsed.changed_ranges,
        old_file_hash,
        new_file_hash,
        diagnostics,
        refreshed_syntax,
        refreshed_locators,
        token_size,
    })
}

/// Parses and validates proposed file content without creating a file.
///
/// # Errors
///
/// Returns syntax/provider failures or [`EditError::ParseRejected`] when the
/// parsed content violates `policy`.
pub fn validate_create_content<P>(
    engine: &SyntaxEngine<P>,
    language_id: LanguageId,
    source: Arc<[u8]>,
    generation: Generation,
    policy: ParsePolicy,
) -> Result<ValidatedCreate, EditError>
where
    P: GrammarProvider,
{
    let snapshot = engine.parse(language_id, Arc::clone(&source), generation)?;
    let diagnostics = EditDiagnostics {
        before_total: 0,
        after_total: snapshot.diagnostic_total(),
        before_truncated: false,
        after_truncated: snapshot.diagnostics_truncated(),
        after: snapshot.diagnostics().to_vec(),
    };
    enforce_parse_policy(policy, &diagnostics, snapshot.file_hash())?;
    let content_hash = snapshot.file_hash();
    let source_bytes = source.len();
    Ok(ValidatedCreate {
        source,
        snapshot,
        content_hash,
        diagnostics,
        token_size: TokenSizeMetadata {
            source_bytes,
            changed_bytes: source_bytes,
            compact_syntax_bytes: 0,
            refreshed_locator_bytes: 0,
            approximate_context_tokens: source_bytes.div_ceil(BYTES_PER_APPROXIMATE_TOKEN),
        },
    })
}

fn operation_geometry(
    operation: &EditOperation,
    node_start: usize,
    node_end: usize,
) -> (usize, usize, &[u8]) {
    match operation {
        EditOperation::Replace(content) => (node_start, node_end, content.as_bytes()),
        EditOperation::Delete => (node_start, node_end, &[]),
        EditOperation::InsertBefore(content) => (node_start, node_start, content.as_bytes()),
        EditOperation::InsertAfter(content) => (node_end, node_end, content.as_bytes()),
    }
}

fn splice(
    source: &[u8],
    start: usize,
    old_end: usize,
    replacement: &[u8],
) -> Result<Vec<u8>, EditError> {
    let retained = source
        .len()
        .checked_sub(
            old_end
                .checked_sub(start)
                .ok_or(EditError::SourceSizeOverflow)?,
        )
        .ok_or(EditError::SourceSizeOverflow)?;
    let new_len = retained
        .checked_add(replacement.len())
        .ok_or(EditError::SourceSizeOverflow)?;
    let mut proposed = Vec::with_capacity(new_len);
    proposed.extend_from_slice(&source[..start]);
    proposed.extend_from_slice(replacement);
    proposed.extend_from_slice(&source[old_end..]);
    Ok(proposed)
}

fn point_at(source: &[u8], offset: usize) -> SourcePoint {
    let prefix = &source[..offset];
    let (row, column) = prefix.iter().fold((0_u64, 0_u64), |(row, column), byte| {
        if *byte == b'\n' {
            (row + 1, 0)
        } else {
            (row, column + 1)
        }
    });
    SourcePoint::new(row, column)
}

fn portable_offset(value: usize) -> Result<u64, EditError> {
    u64::try_from(value).map_err(|_| EditError::SourceOffsetOverflow)
}

fn diagnostics(before: &SyntaxSnapshot, after: &SyntaxSnapshot) -> EditDiagnostics {
    EditDiagnostics {
        before_total: before.diagnostic_total(),
        after_total: after.diagnostic_total(),
        before_truncated: before.diagnostics_truncated(),
        after_truncated: after.diagnostics_truncated(),
        after: after.diagnostics().to_vec(),
    }
}

fn enforce_parse_policy(
    policy: ParsePolicy,
    diagnostics: &EditDiagnostics,
    proposed_file_hash: ContentHash,
) -> Result<(), EditError> {
    let rejected = match policy {
        ParsePolicy::RequireClean => diagnostics.after_total > 0,
        ParsePolicy::NoAdditionalDiagnostics => diagnostics.after_total > diagnostics.before_total,
        ParsePolicy::AllowErrors => false,
    };
    if rejected {
        return Err(EditError::ParseRejected {
            policy,
            before_total: diagnostics.before_total,
            after_total: diagnostics.after_total,
            proposed_file_hash,
            diagnostics: diagnostics.after.clone(),
        });
    }
    Ok(())
}

fn minimal_diff(before: &[u8], after: &[u8]) -> Result<SourceDiff, EditError> {
    let mut prefix = before
        .iter()
        .zip(after)
        .take_while(|(left, right)| left == right)
        .count();
    let valid_utf8 = std::str::from_utf8(before)
        .ok()
        .zip(std::str::from_utf8(after).ok());
    if let Some((before_text, after_text)) = valid_utf8 {
        while prefix > 0
            && (!before_text.is_char_boundary(prefix) || !after_text.is_char_boundary(prefix))
        {
            prefix -= 1;
        }
    }

    let max_suffix = before
        .len()
        .saturating_sub(prefix)
        .min(after.len().saturating_sub(prefix));
    let mut suffix = before[before.len() - max_suffix..]
        .iter()
        .rev()
        .zip(after[after.len() - max_suffix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    if let Some((before_text, after_text)) = valid_utf8 {
        while suffix > 0
            && (!before_text.is_char_boundary(before.len() - suffix)
                || !after_text.is_char_boundary(after.len() - suffix))
        {
            suffix -= 1;
        }
    }

    let old_end = before.len() - suffix;
    let new_end = after.len() - suffix;
    let old_span = ByteSpan::new(portable_offset(prefix)?, portable_offset(old_end)?)
        .map_err(|source| EditError::InvalidIdentity { source })?;
    let new_span = ByteSpan::new(portable_offset(prefix)?, portable_offset(new_end)?)
        .map_err(|source| EditError::InvalidIdentity { source })?;
    let removed = &before[prefix..old_end];
    let inserted = Arc::<[u8]>::from(&after[prefix..new_end]);

    Ok(SourceDiff {
        old_span,
        new_span,
        removed_hash: ContentHash::of(removed),
        inserted_hash: ContentHash::of(&inserted),
        inserted,
    })
}

fn stale_view_request(snapshot: &SyntaxSnapshot, locator: &NodeLocator) -> InspectRequest {
    let source_len = u64::try_from(snapshot.source().len()).unwrap_or(u64::MAX);
    let range = locator.anchor.source_span.bytes;
    InspectRequest {
        max_nodes: DEFAULT_REFRESH_NODES,
        byte_range: (range.end <= source_len).then_some(range),
        ..InspectRequest::default()
    }
}

fn token_size(
    source_bytes: usize,
    changed_bytes: usize,
    inspection: &SyntaxInspection,
    locators: &[NodeLocator],
) -> Result<TokenSizeMetadata, EditError> {
    let compact_syntax_bytes = serde_json::to_vec(inspection)
        .map_err(|source| EditError::MetadataEncoding { source })?
        .len();
    let refreshed_locator_bytes = serde_json::to_vec(locators)
        .map_err(|source| EditError::MetadataEncoding { source })?
        .len();
    let context_bytes = compact_syntax_bytes
        .checked_add(refreshed_locator_bytes)
        .and_then(|value| value.checked_add(changed_bytes))
        .ok_or(EditError::SourceSizeOverflow)?;
    Ok(TokenSizeMetadata {
        source_bytes,
        changed_bytes,
        compact_syntax_bytes,
        refreshed_locator_bytes,
        approximate_context_tokens: context_bytes.div_ceil(BYTES_PER_APPROXIMATE_TOKEN),
    })
}
