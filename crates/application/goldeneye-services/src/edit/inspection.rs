use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use goldeneye_domain::{ContentHash, FileContext, FileId};
use goldeneye_edit::{
    DurableEditError, DurableEditService,
    path_auth::{PathAuthorizer, PathIntent},
};
use goldeneye_ports::{
    EditSyntaxInspect, EditSyntaxInspectRequest, LanguageClassifier, ServiceSyntax,
};

use crate::{Generation, LanguageId, NodeLocator, ServiceError, ServiceErrorCode};

use super::{
    BYTES_PER_APPROXIMATE_TOKEN, InspectSyntaxRequest, InspectSyntaxResult, InspectionSize,
    results::edit_diagnostic_result,
};

pub(super) fn inspect_file(
    service: &mut DurableEditService,
    request: &InspectSyntaxRequest,
    allowed_roots: Vec<PathBuf>,
    languages: &dyn LanguageClassifier,
    syntax: &dyn ServiceSyntax,
) -> Result<InspectSyntaxResult, ServiceError> {
    let (generation, source) = inspection_source(service, request, allowed_roots)?;
    let language_id = inspection_language(languages, request)?;
    let source_bytes = source.len();
    let parsed = syntax
        .inspect(EditSyntaxInspectRequest {
            language_id: language_id.clone(),
            source: Arc::<[u8]>::from(source),
            generation,
            file_context: FileContext::new(request.project.clone(), request.path.clone()),
            inspection: request.inspect.clone(),
        })
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?;
    let size = inspection_size(source_bytes, &parsed)?;
    Ok(inspection_result(request, language_id, parsed, size))
}

fn inspection_source(
    service: &mut DurableEditService,
    request: &InspectSyntaxRequest,
    allowed_roots: Vec<PathBuf>,
) -> Result<(Generation, Vec<u8>), ServiceError> {
    let project = service.indexed_project(&request.project)?.ok_or_else(|| {
        ServiceError::edit(
            ServiceErrorCode::NotFound,
            format!("project is not indexed: {}", request.project.as_str()),
        )
    })?;
    let file_id = FileId::new(request.project.clone(), request.path.clone());
    let indexed = service.indexed_file(&file_id)?.ok_or_else(|| {
        ServiceError::edit(
            ServiceErrorCode::NotFound,
            format!("file is not indexed: {}", request.path.as_str()),
        )
    })?;
    let source = read_authorized_source(&project.root_path, request.path.as_str(), allowed_roots)?;
    let actual_hash = ContentHash::of(&source);
    if actual_hash != indexed.content_hash {
        return Err(ServiceError::from(DurableEditError::StaleSource {
            expected: indexed.content_hash,
            actual: actual_hash,
        }));
    }
    Ok((project.generation, source))
}

fn read_authorized_source(
    project_root: &str,
    relative_path: &str,
    allowed_roots: Vec<PathBuf>,
) -> Result<Vec<u8>, ServiceError> {
    let authorizer = PathAuthorizer::new(allowed_roots).map_err(DurableEditError::from)?;
    let authorized_path = authorizer
        .authorize(project_root, relative_path, PathIntent::Update)
        .map_err(DurableEditError::from)?;
    let destination = authorized_path
        .revalidate()
        .map_err(DurableEditError::from)?;
    fs::read(destination.as_path())
        .map_err(|source| DurableEditError::Io {
            operation: "reading syntax source",
            path: destination.as_path().to_path_buf(),
            source,
        })
        .map_err(ServiceError::from)
}

fn inspection_language(
    languages: &dyn LanguageClassifier,
    request: &InspectSyntaxRequest,
) -> Result<LanguageId, ServiceError> {
    languages
        .classify(Path::new(request.path.as_str()))
        .ok_or_else(|| {
            ServiceError::edit(
                ServiceErrorCode::InvalidInput,
                format!(
                    "cannot detect supported language for {}",
                    request.path.as_str()
                ),
            )
        })
}

fn inspection_size(
    source_bytes: usize,
    parsed: &EditSyntaxInspect,
) -> Result<InspectionSize, ServiceError> {
    let compact_syntax_bytes = serde_json::to_vec(&parsed.inspection)
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?
        .len();
    let locator_bytes = serde_json::to_vec(&parsed.locators)
        .map_err(|error| ServiceError::edit(ServiceErrorCode::InvalidInput, error.to_string()))?
        .len();
    let approximate_context_tokens = source_bytes
        .saturating_add(compact_syntax_bytes)
        .saturating_add(locator_bytes)
        .div_ceil(BYTES_PER_APPROXIMATE_TOKEN);
    Ok(InspectionSize {
        source_bytes,
        compact_syntax_bytes,
        locator_bytes,
        approximate_context_tokens,
    })
}

fn inspection_result(
    request: &InspectSyntaxRequest,
    language_id: LanguageId,
    parsed: EditSyntaxInspect,
    size: InspectionSize,
) -> InspectSyntaxResult {
    InspectSyntaxResult {
        project: request.project.clone(),
        path: request.path.clone(),
        language_id,
        file_hash: parsed.content_hash,
        generation: parsed.generation,
        syntax: parsed.inspection,
        locators: parsed.locators,
        diagnostic_total: parsed.diagnostic_total,
        diagnostics_truncated: parsed.diagnostics_truncated,
        diagnostics: parsed
            .diagnostics
            .iter()
            .map(edit_diagnostic_result)
            .collect(),
        size,
    }
}

pub(super) fn edit_error_with_fresh(
    service: &mut DurableEditService,
    locator: &NodeLocator,
    error: DurableEditError,
    allowed_roots: Vec<PathBuf>,
    languages: &dyn LanguageClassifier,
    syntax: &dyn ServiceSyntax,
) -> ServiceError {
    if !matches!(
        &error,
        DurableEditError::StaleGeneration { .. } | DurableEditError::StaleSource { .. }
    ) {
        return ServiceError::from(error);
    }
    let request = InspectSyntaxRequest::new(
        locator.scope.file.project_id.clone(),
        locator.scope.file.relative_path.clone(),
    );
    let fresh = inspect_file(service, &request, allowed_roots, languages, syntax)
        .and_then(|result| {
            serde_json::to_string(&result.syntax).map_err(|serialization| {
                ServiceError::edit(ServiceErrorCode::Storage, serialization.to_string())
            })
        })
        .unwrap_or_else(|fresh_error| format!("<unavailable:{fresh_error}>"));
    ServiceError::edit(
        ServiceErrorCode::Conflict,
        format!("{error}; fresh_syntax={fresh}"),
    )
}
