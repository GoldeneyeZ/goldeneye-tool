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
    EditSyntaxInspect, EditSyntaxInspectRequest, InspectRequest, LanguageClassifier, ServiceSyntax,
};

use crate::{
    Generation, LanguageId, NodeLocator, ProjectId, ProjectRelativePath, ServiceError,
    ServiceErrorCode,
};

use super::{
    BYTES_PER_APPROXIMATE_TOKEN, CheckSyntaxRequest, CheckSyntaxResult, CheckedFile,
    CheckedFileStatus, InspectSyntaxRequest, InspectSyntaxResult, InspectionSize,
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

/// Parses each requested working-tree file and each supplied text, and reports
/// their diagnostics.
///
/// A file that cannot be read or has no grammar is reported in place, so one
/// bad path does not hide the result of the others.
pub(super) fn check_files(
    project_root: &str,
    generation: Generation,
    request: &CheckSyntaxRequest,
    allowed_roots: &[PathBuf],
    languages: &dyn LanguageClassifier,
    syntax: &dyn ServiceSyntax,
) -> CheckSyntaxResult {
    let from_disk = request.paths.iter().map(|path| {
        let Some(language_id) = languages.classify(Path::new(path.as_str())) else {
            return unchecked_file(path, CheckedFileStatus::Unsupported, None, None);
        };
        match read_authorized_source(project_root, path.as_str(), allowed_roots.to_vec()) {
            Ok(source) => check_source(
                generation,
                &request.project,
                path,
                language_id,
                source,
                syntax,
            ),
            Err(error) => unchecked_file(
                path,
                CheckedFileStatus::Unreadable,
                Some(language_id),
                Some(error.to_string()),
            ),
        }
    });
    let supplied = request.contents.iter().map(|item| {
        let Some(language_id) = languages.classify(Path::new(item.path.as_str())) else {
            return unchecked_file(&item.path, CheckedFileStatus::Unsupported, None, None);
        };
        check_source(
            generation,
            &request.project,
            &item.path,
            language_id,
            item.content.clone().into_bytes(),
            syntax,
        )
    });
    CheckSyntaxResult {
        project: request.project.clone(),
        files: from_disk.chain(supplied).collect(),
    }
}

fn unchecked_file(
    path: &ProjectRelativePath,
    status: CheckedFileStatus,
    language_id: Option<LanguageId>,
    detail: Option<String>,
) -> CheckedFile {
    CheckedFile {
        path: path.clone(),
        status,
        language_id,
        diagnostic_total: 0,
        diagnostics_truncated: false,
        diagnostics: Vec::new(),
        detail,
    }
}

fn check_source(
    generation: Generation,
    project: &ProjectId,
    path: &ProjectRelativePath,
    language_id: LanguageId,
    source: Vec<u8>,
    syntax: &dyn ServiceSyntax,
) -> CheckedFile {
    let parsed = syntax.inspect(EditSyntaxInspectRequest {
        language_id: language_id.clone(),
        source: Arc::<[u8]>::from(source),
        generation,
        file_context: FileContext::new(project.clone(), path.clone()),
        // Only the diagnostics are wanted; one node keeps the traversal minimal.
        inspection: InspectRequest {
            max_depth: 0,
            max_nodes: 1,
            preview_chars: 0,
            byte_range: None,
            node_kinds: Vec::new(),
        },
    });
    match parsed {
        Ok(parsed) => CheckedFile {
            path: path.clone(),
            status: if parsed.diagnostic_total == 0 {
                CheckedFileStatus::Clean
            } else {
                CheckedFileStatus::Errors
            },
            language_id: Some(language_id),
            diagnostic_total: parsed.diagnostic_total,
            diagnostics_truncated: parsed.diagnostics_truncated,
            diagnostics: parsed
                .diagnostics
                .iter()
                .map(edit_diagnostic_result)
                .collect(),
            detail: None,
        },
        Err(error) => unchecked_file(
            path,
            CheckedFileStatus::Unreadable,
            Some(language_id),
            Some(error.to_string()),
        ),
    }
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
