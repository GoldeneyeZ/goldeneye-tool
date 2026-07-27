use std::{fmt::Write as _, ops::Range, path::Path};

use goldeneye_domain::{
    ContentHash, FileId, FileRecord, GraphNode, ProjectId, ProjectRecord, ProjectRelativePath,
    SourceSpan,
};
use goldeneye_ports::QueryRepository;
use sha2::{Digest, Sha256};

use crate::types::{
    CodeSnippetChunkRequest, CodeSnippetChunkResult, CodeSnippetManifestRequest,
    CodeSnippetManifestResult, CodeSnippetRequest, CodeSnippetResult, MAX_SNIPPET_CHUNK_BYTES,
    MIN_SNIPPET_CHUNK_BYTES, QueryError,
};

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
    let loaded = load_snippet(
        repository,
        &request.project,
        &request.qualified_name,
        project,
        graph,
    )?;
    let source_bytes = loaded.source_bytes();
    if source_bytes.len() > request.max_bytes || loaded.source_lines > request.max_lines {
        return Err(QueryError::SnippetTooLarge {
            actual_bytes: source_bytes.len(),
            actual_lines: loaded.source_lines,
            maximum_bytes: request.max_bytes,
            maximum_lines: request.max_lines,
        });
    }
    let source =
        String::from_utf8(source_bytes.to_vec()).map_err(|_| QueryError::SourceNotUtf8 {
            qualified_name: loaded.symbol.qualified_name.as_str().to_owned(),
        })?;
    Ok(CodeSnippetResult {
        project: request.project.as_str().to_owned(),
        symbol: node_summary(loaded.symbol, None, &graph.degrees, Vec::new()),
        source,
        file_path: loaded.file_path.as_str().to_owned(),
        start_byte: loaded.start,
        end_byte: loaded.end,
        start_line: loaded.start_line,
        end_line: loaded.end_line,
        content_hash: hash_hex(&loaded.file.content_hash),
    })
}

pub(super) fn execute_manifest(
    repository: &dyn QueryRepository,
    request: &CodeSnippetManifestRequest,
    project: &ProjectRecord,
    graph: &ProjectGraph,
) -> Result<CodeSnippetManifestResult, QueryError> {
    validate_chunk_bytes(request.chunk_bytes)?;
    let loaded = load_snippet(
        repository,
        &request.project,
        &request.qualified_name,
        project,
        graph,
    )?;
    let source = loaded.source_str()?;
    let source_sha256 = source_sha256(source.as_bytes());
    let chunk_count = chunk_ranges(source, request.chunk_bytes).len();
    Ok(CodeSnippetManifestResult {
        project: request.project.as_str().to_owned(),
        symbol: node_summary(loaded.symbol, None, &graph.degrees, Vec::new()),
        file_path: loaded.file_path.as_str().to_owned(),
        start_byte: loaded.start,
        end_byte: loaded.end,
        start_line: loaded.start_line,
        end_line: loaded.end_line,
        source_bytes: source.len(),
        source_lines: loaded.source_lines,
        source_sha256,
        indexed_file_hash: hash_hex(&loaded.file.content_hash),
        chunk_bytes: request.chunk_bytes,
        chunk_count,
    })
}

pub(super) fn execute_chunk(
    repository: &dyn QueryRepository,
    request: &CodeSnippetChunkRequest,
    project: &ProjectRecord,
    graph: &ProjectGraph,
) -> Result<CodeSnippetChunkResult, QueryError> {
    validate_chunk_bytes(request.chunk_bytes)?;
    validate_expected_source_sha256(request.expected_source_sha256.as_deref())?;
    let loaded = load_snippet(
        repository,
        &request.project,
        &request.qualified_name,
        project,
        graph,
    )?;
    let source = loaded.source_str()?;
    let source_sha256 = source_sha256(source.as_bytes());
    if let Some(expected) = request.expected_source_sha256.as_deref()
        && expected != source_sha256
    {
        return Err(QueryError::StaleSnippetSource {
            expected_source_sha256: expected.to_owned(),
            actual_source_sha256: source_sha256,
        });
    }
    let ranges = chunk_ranges(source, request.chunk_bytes);
    let chunk_count = ranges.len();
    let Some(range) = request
        .chunk
        .checked_sub(1)
        .and_then(|index| ranges.get(index))
        .cloned()
    else {
        return Err(QueryError::InvalidSnippetChunk {
            actual: request.chunk,
            chunk_count,
        });
    };
    let file_chunk_start_byte =
        loaded
            .start
            .checked_add(range.start)
            .ok_or_else(|| QueryError::CorruptSourceSpan {
                qualified_name: loaded.symbol.qualified_name.as_str().to_owned(),
            })?;
    let file_chunk_end_byte =
        loaded
            .start
            .checked_add(range.end)
            .ok_or_else(|| QueryError::CorruptSourceSpan {
                qualified_name: loaded.symbol.qualified_name.as_str().to_owned(),
            })?;
    let chunk_start_line = line_at_offset(source.as_bytes(), loaded.start_line, range.start);
    let chunk_end_line = line_at_offset(
        source.as_bytes(),
        loaded.start_line,
        range.end.saturating_sub(1),
    );
    let eof = request.chunk == chunk_count;
    Ok(CodeSnippetChunkResult {
        project: request.project.as_str().to_owned(),
        symbol: node_summary(loaded.symbol, None, &graph.degrees, Vec::new()),
        source: source[range.clone()].to_owned(),
        file_path: loaded.file_path.as_str().to_owned(),
        start_byte: loaded.start,
        end_byte: loaded.end,
        start_line: loaded.start_line,
        end_line: loaded.end_line,
        source_bytes: source.len(),
        source_lines: loaded.source_lines,
        source_sha256,
        indexed_file_hash: hash_hex(&loaded.file.content_hash),
        chunk_bytes: request.chunk_bytes,
        chunk_count,
        chunk: request.chunk,
        chunk_start_byte: range.start,
        chunk_end_byte: range.end,
        file_chunk_start_byte,
        file_chunk_end_byte,
        chunk_start_line,
        chunk_end_line,
        eof,
        truncated: !eof,
    })
}

struct LoadedSnippet<'graph> {
    symbol: &'graph GraphNode,
    file_path: &'graph ProjectRelativePath,
    file: FileRecord,
    bytes: Vec<u8>,
    start: usize,
    end: usize,
    start_line: u64,
    end_line: u64,
    source_lines: usize,
}

impl LoadedSnippet<'_> {
    fn source_bytes(&self) -> &[u8] {
        &self.bytes[self.start..self.end]
    }

    fn source_str(&self) -> Result<&str, QueryError> {
        std::str::from_utf8(self.source_bytes()).map_err(|_| QueryError::SourceNotUtf8 {
            qualified_name: self.symbol.qualified_name.as_str().to_owned(),
        })
    }
}

fn load_snippet<'graph>(
    repository: &dyn QueryRepository,
    project_id: &ProjectId,
    qualified_name: &str,
    project: &ProjectRecord,
    graph: &'graph ProjectGraph,
) -> Result<LoadedSnippet<'graph>, QueryError> {
    let (symbol, file_path, span) = resolve_source_location(qualified_name, graph)?;
    let file = indexed_file(repository, project_id, graph, file_path)?;
    let bytes = fresh_source(project, file_path, &file)?;
    let range = source_range(&bytes, symbol, span)?;
    let source_lines = source_line_count(&bytes[range.clone()]);
    let start_line = span.start.row + 1;
    let end_line = start_line + u64::try_from(source_lines.saturating_sub(1)).unwrap_or(u64::MAX);
    Ok(LoadedSnippet {
        symbol,
        file_path,
        file,
        bytes,
        start: range.start,
        end: range.end,
        start_line,
        end_line,
        source_lines,
    })
}

fn resolve_source_location<'graph>(
    qualified_name: &str,
    graph: &'graph ProjectGraph,
) -> Result<(&'graph GraphNode, &'graph ProjectRelativePath, SourceSpan), QueryError> {
    let symbol = resolve_symbol_in_graph(qualified_name, graph, ResolveMode::Any)?;
    let file_path = symbol
        .file_path
        .as_ref()
        .ok_or_else(|| QueryError::SourceFileUnavailable {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
        })?;
    let span = symbol
        .source_span
        .ok_or_else(|| QueryError::SourceSpanUnavailable {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
        })?;
    Ok((symbol, file_path, span))
}

fn indexed_file(
    repository: &dyn QueryRepository,
    project: &ProjectId,
    graph: &ProjectGraph,
    file_path: &ProjectRelativePath,
) -> Result<FileRecord, QueryError> {
    if let Some(file) = graph.cached_file(file_path.as_str()) {
        return Ok(file);
    }
    let file = repository
        .get_file(&FileId::new(project.clone(), file_path.clone()))?
        .ok_or_else(|| QueryError::IndexedFileNotFound {
            path: file_path.as_str().to_owned(),
        })?;
    graph.cache_file(file.clone());
    Ok(file)
}

fn fresh_source(
    project: &ProjectRecord,
    file_path: &ProjectRelativePath,
    file: &FileRecord,
) -> Result<Vec<u8>, QueryError> {
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
    Ok(bytes)
}

fn source_range(
    bytes: &[u8],
    symbol: &GraphNode,
    span: SourceSpan,
) -> Result<Range<usize>, QueryError> {
    let start = usize::try_from(span.bytes.start).map_err(|_| QueryError::CorruptSourceSpan {
        qualified_name: symbol.qualified_name.as_str().to_owned(),
    })?;
    let end = usize::try_from(span.bytes.end).map_err(|_| QueryError::CorruptSourceSpan {
        qualified_name: symbol.qualified_name.as_str().to_owned(),
    })?;
    bytes
        .get(start..end)
        .map(|_| start..end)
        .ok_or_else(|| QueryError::CorruptSourceSpan {
            qualified_name: symbol.qualified_name.as_str().to_owned(),
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

fn validate_chunk_bytes(actual: usize) -> Result<(), QueryError> {
    if !(MIN_SNIPPET_CHUNK_BYTES..=MAX_SNIPPET_CHUNK_BYTES).contains(&actual) {
        return Err(QueryError::InvalidSnippetChunkBytes {
            actual,
            minimum: MIN_SNIPPET_CHUNK_BYTES,
            maximum: MAX_SNIPPET_CHUNK_BYTES,
        });
    }
    Ok(())
}

fn validate_expected_source_sha256(expected: Option<&str>) -> Result<(), QueryError> {
    let Some(expected) = expected else {
        return Ok(());
    };
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(QueryError::InvalidSnippetArguments {
            field: "expected_source_sha256",
            reason: "must be exactly 64 lowercase hexadecimal characters",
        });
    }
    Ok(())
}

fn chunk_ranges(source: &str, chunk_bytes: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < source.len() {
        let mut end = start.saturating_add(chunk_bytes).min(source.len());
        while end > start && !source.is_char_boundary(end) {
            end -= 1;
        }
        debug_assert!(end > start);
        ranges.push(start..end);
        start = end;
    }
    ranges
}

fn line_at_offset(source: &[u8], start_line: u64, offset: usize) -> u64 {
    start_line
        + u64::try_from(
            source[..offset.min(source.len())]
                .split(|byte| *byte == b'\n')
                .count()
                .saturating_sub(1),
        )
        .unwrap_or(u64::MAX)
}

fn source_line_count(source: &[u8]) -> usize {
    if source.is_empty() {
        return 0;
    }
    source.split(|byte| *byte == b'\n').count() - usize::from(source.ends_with(b"\n"))
}

fn source_sha256(source: &[u8]) -> String {
    let digest = Sha256::digest(source);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn hash_hex(hash: &ContentHash) -> String {
    let mut encoded = String::with_capacity(hash.as_bytes().len() * 2);
    for byte in hash.as_bytes() {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::chunk_ranges;

    #[test]
    fn chunks_use_largest_utf8_boundary_without_gaps() {
        let source = format!("{}🦀{}", "a".repeat(255), "b".repeat(300));
        let ranges = chunk_ranges(&source, 256);
        assert_eq!(ranges[0], 0..255);
        assert!(ranges.iter().all(|range| range.len() <= 256));
        assert!(ranges.windows(2).all(|pair| pair[0].end == pair[1].start));
        let rebuilt = ranges
            .iter()
            .map(|range| &source[range.clone()])
            .collect::<String>();
        assert_eq!(rebuilt, source);
    }

    #[test]
    fn chunks_split_long_lines_and_exact_boundaries_deterministically() {
        let source = "x".repeat(512);
        let first = chunk_ranges(&source, 256);
        let second = chunk_ranges(&source, 256);
        assert_eq!(first, vec![0..256, 256..512]);
        assert_eq!(first, second);
    }

    #[test]
    fn empty_source_has_no_chunks() {
        assert!(chunk_ranges("", 256).is_empty());
    }
}
