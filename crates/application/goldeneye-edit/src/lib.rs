#![forbid(unsafe_code)]

mod durable;
pub mod path_auth;

pub use durable::{
    DurableCreateRequest, DurableEditError, DurableEditRequest, DurableEditService, FaultInjector,
    FaultPoint, GraphChanges, MutationResult, RecoveryAction, RecoveryEntry, RecoveryReport,
};

use std::sync::Arc;

use goldeneye_domain::{
    ByteSpan, ContentHash, FileContext, Generation, LanguageId, NodeLocator, SyntaxIdentityError,
};
use goldeneye_ports::{
    EditInspectRequest, EditSyntax, EditSyntaxCreateRequest, EditSyntaxDiagnostic, EditSyntaxError,
    EditSyntaxInspection, EditSyntaxMutation, EditSyntaxPlanRequest, PortError,
};
use thiserror::Error;

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
    pub refresh_request: EditInspectRequest,
}

impl Default for EditOptions {
    fn default() -> Self {
        Self {
            parse_policy: ParsePolicy::RequireClean,
            refresh_request: EditInspectRequest::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditPlanRequest {
    pub language_id: LanguageId,
    pub source: Arc<[u8]>,
    pub current_generation: Generation,
    pub file_context: FileContext,
    pub locator: NodeLocator,
    pub operation: EditOperation,
    pub next_generation: Generation,
    pub options: EditOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditDiagnostics {
    pub before_total: usize,
    pub after_total: usize,
    pub before_truncated: bool,
    pub after_truncated: bool,
    pub after: Vec<EditSyntaxDiagnostic>,
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
    pub diff: SourceDiff,
    pub changed_ranges: Vec<goldeneye_domain::SourceSpan>,
    pub old_file_hash: ContentHash,
    pub new_file_hash: ContentHash,
    pub diagnostics: EditDiagnostics,
    pub refreshed_syntax: EditSyntaxInspection,
    pub refreshed_locators: Vec<NodeLocator>,
    pub token_size: TokenSizeMetadata,
}

pub struct ValidatedCreate {
    pub source: Arc<[u8]>,
    pub content_hash: ContentHash,
    pub diagnostics: EditDiagnostics,
    pub locators: Vec<NodeLocator>,
    pub token_size: TokenSizeMetadata,
}

#[derive(Debug, Error)]
pub enum EditError {
    #[error("node locator is stale: {cause}")]
    StaleLocator {
        cause: String,
        fresh: Box<EditSyntaxInspection>,
    },
    #[error(
        "proposed source rejected by {policy:?}: {after_total} diagnostics after {before_total}"
    )]
    ParseRejected {
        policy: ParsePolicy,
        before_total: usize,
        after_total: usize,
        proposed_file_hash: ContentHash,
        diagnostics: Vec<EditSyntaxDiagnostic>,
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
    Syntax(#[from] PortError),
}

/// Plans one exact named-node mutation without writing to the filesystem.
///
/// # Errors
///
/// Returns a typed stale-locator error with fresh syntax context when any
/// identity guard fails. Syntax, inspection, parse-policy, size, identity, and
/// metadata failures are returned without mutating source.
pub fn plan_edit(
    syntax: &dyn EditSyntax,
    request: &EditPlanRequest,
) -> Result<EditPlan, EditError> {
    let planned = syntax
        .plan_edit(EditSyntaxPlanRequest {
            language_id: request.language_id.clone(),
            source: Arc::clone(&request.source),
            current_generation: request.current_generation,
            file_context: request.file_context.clone(),
            locator: request.locator.clone(),
            operation: (&request.operation).into(),
            next_generation: request.next_generation,
            inspection: request.options.refresh_request.clone(),
        })
        .map_err(EditError::from)?;
    let diagnostics = EditDiagnostics {
        before_total: planned.before_diagnostic_total,
        after_total: planned.after_diagnostic_total,
        before_truncated: planned.before_diagnostics_truncated,
        after_truncated: planned.after_diagnostics_truncated,
        after: planned.diagnostics,
    };
    enforce_parse_policy(
        request.options.parse_policy,
        &diagnostics,
        planned.new_file_hash,
    )?;

    let diff = minimal_diff(&request.source, &planned.source)?;
    let token_size = token_size(
        planned.source.len(),
        diff.inserted.len(),
        &planned.inspection,
        &planned.locators,
    )?;

    Ok(EditPlan {
        source: planned.source,
        diff,
        changed_ranges: planned.changed_ranges,
        old_file_hash: planned.old_file_hash,
        new_file_hash: planned.new_file_hash,
        diagnostics,
        refreshed_syntax: planned.inspection,
        refreshed_locators: planned.locators,
        token_size,
    })
}

/// Parses and validates proposed file content without creating a file.
///
/// # Errors
///
/// Returns syntax/provider failures or [`EditError::ParseRejected`] when the
/// parsed content violates `policy`.
pub fn validate_create_content(
    syntax: &dyn EditSyntax,
    language_id: LanguageId,
    source: Arc<[u8]>,
    generation: Generation,
    file_context: &FileContext,
    policy: ParsePolicy,
) -> Result<ValidatedCreate, EditError> {
    let parsed = syntax.parse_create(EditSyntaxCreateRequest {
        language_id,
        source,
        generation,
        file_context: file_context.clone(),
    })?;
    let diagnostics = EditDiagnostics {
        before_total: 0,
        after_total: parsed.diagnostic_total,
        before_truncated: false,
        after_truncated: parsed.diagnostics_truncated,
        after: parsed.diagnostics,
    };
    enforce_parse_policy(policy, &diagnostics, parsed.content_hash)?;
    let source_bytes = parsed.source.len();
    Ok(ValidatedCreate {
        source: parsed.source,
        content_hash: parsed.content_hash,
        diagnostics,
        locators: parsed.locators,
        token_size: TokenSizeMetadata {
            source_bytes,
            changed_bytes: source_bytes,
            compact_syntax_bytes: 0,
            refreshed_locator_bytes: 0,
            approximate_context_tokens: source_bytes.div_ceil(BYTES_PER_APPROXIMATE_TOKEN),
        },
    })
}

fn portable_offset(value: usize) -> Result<u64, EditError> {
    u64::try_from(value).map_err(|_| EditError::SourceOffsetOverflow)
}

impl From<&EditOperation> for EditSyntaxMutation {
    fn from(operation: &EditOperation) -> Self {
        match operation {
            EditOperation::Replace(content) => Self::Replace(content.clone()),
            EditOperation::Delete => Self::Delete,
            EditOperation::InsertBefore(content) => Self::InsertBefore(content.clone()),
            EditOperation::InsertAfter(content) => Self::InsertAfter(content.clone()),
        }
    }
}

impl From<EditSyntaxError> for EditError {
    fn from(error: EditSyntaxError) -> Self {
        match error {
            EditSyntaxError::StaleLocator { cause, fresh } => Self::StaleLocator { cause, fresh },
            EditSyntaxError::Adapter(error) => Self::Syntax(error),
        }
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

fn token_size(
    source_bytes: usize,
    changed_bytes: usize,
    inspection: &EditSyntaxInspection,
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
