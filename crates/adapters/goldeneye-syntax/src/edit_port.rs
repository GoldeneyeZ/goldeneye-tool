use std::sync::Arc;

use goldeneye_domain::{ByteSpan, SourcePoint, SyntaxIdentityError};
use goldeneye_ports::{
    EditDiagnosticKind, EditInspectRequest, EditSyntax, EditSyntaxCreate, EditSyntaxCreateRequest,
    EditSyntaxDiagnostic, EditSyntaxError, EditSyntaxInspection, EditSyntaxMutation,
    EditSyntaxNodeView, EditSyntaxPlan, EditSyntaxPlanRequest, PortError,
};
use thiserror::Error;

use crate::{
    DiagnosticKind, GrammarProvider, InspectRequest, SyntaxDiagnostic, SyntaxEdit, SyntaxEngine,
    SyntaxInspection, SyntaxSnapshot, all_named_locators, inspect_syntax, resolve_locator,
};

#[derive(Debug, Error)]
enum EditPortError {
    #[error("source size arithmetic overflowed")]
    SourceSizeOverflow,
    #[error("source offset cannot be represented by the portable byte model")]
    SourceOffsetOverflow,
    #[error("cannot construct edit identity: {source}")]
    InvalidIdentity {
        #[source]
        source: SyntaxIdentityError,
    },
}

impl<P> EditSyntax for SyntaxEngine<P>
where
    P: GrammarProvider + Send + Sync,
{
    fn plan_edit(&self, request: EditSyntaxPlanRequest) -> Result<EditSyntaxPlan, EditSyntaxError> {
        plan_edit(self, &request)
    }

    fn parse_create(
        &self,
        request: EditSyntaxCreateRequest,
    ) -> Result<EditSyntaxCreate, PortError> {
        parse_create(self, request)
    }
}

fn plan_edit<P>(
    engine: &SyntaxEngine<P>,
    request: &EditSyntaxPlanRequest,
) -> Result<EditSyntaxPlan, EditSyntaxError>
where
    P: GrammarProvider,
{
    let snapshot = engine
        .parse(
            request.language_id.clone(),
            Arc::clone(&request.source),
            request.current_generation,
        )
        .map_err(adapter_error)?;
    let node = match resolve_locator(&snapshot, &request.file_context, &request.locator) {
        Ok(node) => node,
        Err(cause) => {
            let fresh = inspect_syntax(
                &snapshot,
                &request.file_context,
                &stale_inspect_request(&snapshot, request),
            )
            .map(map_inspection)
            .map_err(adapter_error)?;
            return Err(EditSyntaxError::StaleLocator {
                cause: cause.to_string(),
                fresh: Box::new(fresh),
            });
        }
    };

    let node_start = node.start_byte();
    let node_end = node.end_byte();
    let (start, old_end, replacement) =
        operation_geometry(&request.operation, node_start, node_end);
    let proposed = splice(snapshot.source(), start, old_end, replacement).map_err(adapter_error)?;
    let new_end = start
        .checked_add(replacement.len())
        .ok_or(EditPortError::SourceSizeOverflow)
        .map_err(adapter_error)?;
    let syntax_edit = SyntaxEdit::new(
        portable_offset(start).map_err(adapter_error)?,
        portable_offset(old_end).map_err(adapter_error)?,
        portable_offset(new_end).map_err(adapter_error)?,
        point_at(snapshot.source(), start),
        point_at(snapshot.source(), old_end),
        point_at(&proposed, new_end),
    );
    let source = Arc::<[u8]>::from(proposed);
    let reparsed = engine
        .reparse(
            &snapshot,
            snapshot.language_id().clone(),
            Arc::clone(&source),
            request.next_generation,
            syntax_edit,
        )
        .map_err(adapter_error)?;

    let mut inspection_request = map_inspect_request(&request.inspection);
    inspection_request.byte_range =
        Some(minimal_new_span(snapshot.source(), &source).map_err(adapter_error)?);
    let inspection = inspect_syntax(
        &reparsed.snapshot,
        &request.file_context,
        &inspection_request,
    )
    .map_err(adapter_error)?;
    let locators = inspection
        .nodes
        .iter()
        .map(|node| inspection.locator(node.ordinal))
        .collect::<Result<Vec<_>, _>>()
        .map_err(adapter_error)?;

    Ok(EditSyntaxPlan {
        source,
        old_file_hash: snapshot.file_hash(),
        new_file_hash: reparsed.snapshot.file_hash(),
        changed_ranges: reparsed.changed_ranges,
        before_diagnostic_total: snapshot.diagnostic_total(),
        before_diagnostics_truncated: snapshot.diagnostics_truncated(),
        after_diagnostic_total: reparsed.snapshot.diagnostic_total(),
        after_diagnostics_truncated: reparsed.snapshot.diagnostics_truncated(),
        diagnostics: map_diagnostics(reparsed.snapshot.diagnostics()),
        inspection: map_inspection(inspection),
        locators,
    })
}

fn parse_create<P>(
    engine: &SyntaxEngine<P>,
    request: EditSyntaxCreateRequest,
) -> Result<EditSyntaxCreate, PortError>
where
    P: GrammarProvider,
{
    let snapshot = engine
        .parse(
            request.language_id,
            Arc::clone(&request.source),
            request.generation,
        )
        .map_err(PortError::new)?;
    let locators = all_named_locators(&snapshot, &request.file_context).map_err(PortError::new)?;

    Ok(EditSyntaxCreate {
        source: request.source,
        content_hash: snapshot.file_hash(),
        diagnostic_total: snapshot.diagnostic_total(),
        diagnostics_truncated: snapshot.diagnostics_truncated(),
        diagnostics: map_diagnostics(snapshot.diagnostics()),
        locators,
    })
}

fn operation_geometry(
    operation: &EditSyntaxMutation,
    node_start: usize,
    node_end: usize,
) -> (usize, usize, &[u8]) {
    match operation {
        EditSyntaxMutation::Replace(content) => (node_start, node_end, content.as_bytes()),
        EditSyntaxMutation::Delete => (node_start, node_end, &[]),
        EditSyntaxMutation::InsertBefore(content) => (node_start, node_start, content.as_bytes()),
        EditSyntaxMutation::InsertAfter(content) => (node_end, node_end, content.as_bytes()),
    }
}

fn splice(
    source: &[u8],
    start: usize,
    old_end: usize,
    replacement: &[u8],
) -> Result<Vec<u8>, EditPortError> {
    let retained = source
        .len()
        .checked_sub(
            old_end
                .checked_sub(start)
                .ok_or(EditPortError::SourceSizeOverflow)?,
        )
        .ok_or(EditPortError::SourceSizeOverflow)?;
    let new_len = retained
        .checked_add(replacement.len())
        .ok_or(EditPortError::SourceSizeOverflow)?;
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

fn portable_offset(value: usize) -> Result<u64, EditPortError> {
    u64::try_from(value).map_err(|_| EditPortError::SourceOffsetOverflow)
}

fn minimal_new_span(before: &[u8], after: &[u8]) -> Result<ByteSpan, EditPortError> {
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

    let new_end = after.len() - suffix;
    ByteSpan::new(portable_offset(prefix)?, portable_offset(new_end)?)
        .map_err(|source| EditPortError::InvalidIdentity { source })
}

fn stale_inspect_request(
    snapshot: &SyntaxSnapshot,
    request: &EditSyntaxPlanRequest,
) -> InspectRequest {
    let mut inspection = map_inspect_request(&request.inspection);
    let source_len = u64::try_from(snapshot.source().len()).unwrap_or(u64::MAX);
    let range = request.locator.anchor.source_span.bytes;
    inspection.byte_range = (range.end <= source_len).then_some(range);
    inspection
}

fn map_inspect_request(request: &EditInspectRequest) -> InspectRequest {
    InspectRequest {
        max_depth: request.max_depth,
        max_nodes: request.max_nodes,
        preview_chars: request.preview_chars,
        byte_range: request.byte_range,
        node_kinds: request.node_kinds.clone(),
    }
}

fn map_diagnostics(diagnostics: &[SyntaxDiagnostic]) -> Vec<EditSyntaxDiagnostic> {
    diagnostics
        .iter()
        .map(|diagnostic| EditSyntaxDiagnostic {
            kind: match diagnostic.kind {
                DiagnosticKind::Error => EditDiagnosticKind::Error,
                DiagnosticKind::Missing => EditDiagnosticKind::Missing,
            },
            node_kind: diagnostic.node_kind.clone(),
            span: diagnostic.span,
        })
        .collect()
}

fn map_inspection(inspection: SyntaxInspection) -> EditSyntaxInspection {
    EditSyntaxInspection {
        scope: inspection.scope,
        base_ancestor_path: inspection.base_ancestor_path,
        nodes: inspection
            .nodes
            .into_iter()
            .map(|node| EditSyntaxNodeView {
                ordinal: node.ordinal,
                parent_ordinal: node.parent_ordinal,
                depth: node.depth,
                named_child_index: node.named_child_index,
                field_name: node.field_name,
                kind: node.kind,
                span: node.span,
                content_hash: node.content_hash,
                named_child_count: node.named_child_count,
                preview: node.preview,
                locator_path: node.locator_path,
            })
            .collect(),
        truncated: inspection.truncated,
        total_named_nodes_seen: inspection.total_named_nodes_seen,
    }
}

fn adapter_error(error: impl std::error::Error + Send + Sync + 'static) -> EditSyntaxError {
    EditSyntaxError::Adapter(PortError::new(error))
}
