use goldeneye_edit::{
    DurableEditError, EditError, MutationResult as DurableMutationResult,
    path_auth::PathAuthorizationError,
};
use goldeneye_ports::{EditDiagnosticKind, EditSyntaxDiagnostic};

use crate::{ServiceError, ServiceErrorCode};

use super::{
    EditMutationResult, GraphMutation, MutationDiagnostics, MutationDiff, MutationSize,
    SyntaxDiagnosticResult,
};

pub(super) fn mutation_result(result: DurableMutationResult) -> EditMutationResult {
    let diagnostics = mutation_diagnostics(&result);
    EditMutationResult {
        operation_id: result.operation_id,
        project: result.project_id,
        path: result.relative_path,
        old_file_hash: result.old_file_hash,
        new_file_hash: result.new_file_hash,
        diff: MutationDiff {
            old_span: result.diff.old_span,
            new_span: result.diff.new_span,
            removed_hash: result.diff.removed_hash,
            inserted_hash: result.diff.inserted_hash,
            inserted_bytes: result.diff.inserted.len(),
        },
        changed_syntax_ids: result.syntax_identities,
        changed_graph_ids: result.changed_graph_identities,
        graph: GraphMutation {
            added: result.graph_changes.added,
            removed: result.graph_changes.removed,
            retained: result.graph_changes.retained,
        },
        generation: result.generation,
        diagnostics,
        size: MutationSize {
            source_bytes: result.token_size.source_bytes,
            changed_bytes: result.token_size.changed_bytes,
            compact_syntax_bytes: result.token_size.compact_syntax_bytes,
            refreshed_locator_bytes: result.token_size.refreshed_locator_bytes,
            approximate_context_tokens: result.token_size.approximate_context_tokens,
        },
    }
}

fn mutation_diagnostics(result: &DurableMutationResult) -> MutationDiagnostics {
    MutationDiagnostics {
        before_total: result.diagnostics.before_total,
        after_total: result.diagnostics.after_total,
        before_truncated: result.diagnostics.before_truncated,
        after_truncated: result.diagnostics.after_truncated,
        after: result
            .diagnostics
            .after
            .iter()
            .map(edit_diagnostic_result)
            .collect(),
    }
}

pub(super) fn edit_diagnostic_result(diagnostic: &EditSyntaxDiagnostic) -> SyntaxDiagnosticResult {
    SyntaxDiagnosticResult {
        kind: match diagnostic.kind {
            EditDiagnosticKind::Error => "error",
            EditDiagnosticKind::Missing => "missing",
        }
        .to_owned(),
        node_kind: diagnostic.node_kind.clone(),
        span: diagnostic.span,
    }
}

impl ServiceError {
    pub(crate) fn edit(code: ServiceErrorCode, message: impl Into<String>) -> Self {
        Self::Edit {
            code,
            message: message.into(),
        }
    }
}

impl From<DurableEditError> for ServiceError {
    fn from(error: DurableEditError) -> Self {
        if let DurableEditError::Edit(EditError::StaleLocator { cause, fresh }) = error {
            let fresh = serde_json::to_string(&fresh)
                .unwrap_or_else(|serialization| format!("<unavailable:{serialization}>"));
            return Self::edit(
                ServiceErrorCode::Conflict,
                format!("node locator is stale: {cause}; fresh_syntax={fresh}"),
            );
        }
        let code = durable_edit_error_code(&error);
        Self::edit(code, error.to_string())
    }
}

fn durable_edit_error_code(error: &DurableEditError) -> ServiceErrorCode {
    match error {
        DurableEditError::Path(PathAuthorizationError::DestinationExists { .. })
        | DurableEditError::StaleGeneration { .. }
        | DurableEditError::StaleSource { .. }
        | DurableEditError::TargetBusy(_)
        | DurableEditError::NoContentChange
        | DurableEditError::RecoveryRequired { .. } => ServiceErrorCode::Conflict,
        DurableEditError::Path(
            PathAuthorizationError::ProjectOutsideAllowedRoots { .. }
            | PathAuthorizationError::ProjectRootChanged { .. }
            | PathAuthorizationError::PathEscapesProject { .. }
            | PathAuthorizationError::ReservedMetadata { .. },
        ) => ServiceErrorCode::Forbidden,
        DurableEditError::Path(
            PathAuthorizationError::DestinationMissing { .. }
            | PathAuthorizationError::DestinationNotFile { .. },
        )
        | DurableEditError::ProjectNotFound(_)
        | DurableEditError::FileNotIndexed(_)
        | DurableEditError::OperationNotFound(_) => ServiceErrorCode::NotFound,
        DurableEditError::Repository(_) | DurableEditError::Io { .. } => ServiceErrorCode::Storage,
        DurableEditError::RefreshRejected { .. } => ServiceErrorCode::Index,
        DurableEditError::InjectedFault { .. }
        | DurableEditError::JournalPathMismatch(_)
        | DurableEditError::Path(_)
        | DurableEditError::Edit(_)
        | DurableEditError::Identity(_)
        | DurableEditError::GenerationOverflow(_) => ServiceErrorCode::InvalidInput,
    }
}
