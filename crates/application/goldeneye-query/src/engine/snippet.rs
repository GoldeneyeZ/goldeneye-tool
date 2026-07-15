use std::{fmt::Write as _, path::Path};

use goldeneye_domain::{ContentHash, FileId, ProjectRecord};
use goldeneye_ports::QueryRepository;

use crate::types::{CodeSnippetRequest, CodeSnippetResult, QueryError};

use super::{ProjectGraph, ResolveMode, node_summary, resolve_symbol_in_graph};

const MAX_SNIPPET_BYTES: usize = 1_048_576;
const MAX_SNIPPET_LINES: usize = 10_000;

pub(super) fn execute(
    repository: &dyn QueryRepository,
    request: &CodeSnippetRequest,
    project: &ProjectRecord,
    graph: &ProjectGraph,
) -> Result<CodeSnippetResult, QueryError> {
    validate_limit("max_bytes", request.max_bytes, MAX_SNIPPET_BYTES)?;
    validate_limit("max_lines", request.max_lines, MAX_SNIPPET_LINES)?;
    let symbol = resolve_symbol_in_graph(&request.qualified_name, graph, ResolveMode::Any)?;
    let file_path = symbol
        .file_path
        .clone()
        .ok_or_else(|| QueryError::SourceFileUnavailable {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
        })?;
    let span = symbol
        .source_span
        .ok_or_else(|| QueryError::SourceSpanUnavailable {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
        })?;
    let file = if let Some(file) = graph.cached_file(file_path.as_str()) {
        file
    } else {
        let file = repository
            .get_file(&FileId::new(request.project.clone(), file_path.clone()))?
            .ok_or_else(|| QueryError::IndexedFileNotFound {
                path: file_path.as_str().to_owned(),
            })?;
        graph.cache_file(file.clone());
        file
    };
    let absolute_path = Path::new(&project.root_path).join(file_path.as_str());
    let bytes = std::fs::read(&absolute_path).map_err(|source| QueryError::SourceRead {
        path: absolute_path,
        source,
    })?;
    let actual_hash = ContentHash::of(&bytes);
    if actual_hash != file.content_hash {
        return Err(QueryError::StaleFile {
            path: file_path.as_str().to_owned(),
            expected_hash: hash_hex(&file.content_hash),
            actual_hash: hash_hex(&actual_hash),
        });
    }
    let start = usize::try_from(span.bytes.start).map_err(|_| QueryError::CorruptSourceSpan {
        qualified_name: symbol.qualified_name.as_str().to_owned(),
    })?;
    let end = usize::try_from(span.bytes.end).map_err(|_| QueryError::CorruptSourceSpan {
        qualified_name: symbol.qualified_name.as_str().to_owned(),
    })?;
    let source_bytes = bytes
        .get(start..end)
        .ok_or_else(|| QueryError::CorruptSourceSpan {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
        })?;
    let line_count = source_line_count(source_bytes);
    if source_bytes.len() > request.max_bytes || line_count > request.max_lines {
        return Err(QueryError::SnippetTooLarge {
            actual_bytes: source_bytes.len(),
            actual_lines: line_count,
            maximum_bytes: request.max_bytes,
            maximum_lines: request.max_lines,
        });
    }
    let source =
        String::from_utf8(source_bytes.to_vec()).map_err(|_| QueryError::SourceNotUtf8 {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
        })?;
    let start_line = span.start.row + 1;
    let end_line = start_line + u64::try_from(line_count.saturating_sub(1)).unwrap_or(u64::MAX);
    Ok(CodeSnippetResult {
        project: request.project.as_str().to_owned(),
        symbol: node_summary(&symbol, None, &graph.degrees, Vec::new()),
        source,
        file_path: file_path.as_str().to_owned(),
        start_byte: start,
        end_byte: end,
        start_line,
        end_line,
        content_hash: hash_hex(&file.content_hash),
    })
}

fn validate_limit(field: &'static str, actual: usize, maximum: usize) -> Result<(), QueryError> {
    if actual == 0 || actual > maximum {
        return Err(QueryError::InvalidSnippetLimit {
            field,
            actual,
            maximum,
        });
    }
    Ok(())
}

fn source_line_count(source: &[u8]) -> usize {
    if source.is_empty() {
        return 0;
    }
    source.split(|byte| *byte == b'\n').count() - usize::from(source.ends_with(b"\n"))
}

fn hash_hex(hash: &ContentHash) -> String {
    let mut encoded = String::with_capacity(hash.as_bytes().len() * 2);
    for byte in hash.as_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}
