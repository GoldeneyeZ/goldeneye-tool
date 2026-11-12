use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    num::NonZeroUsize,
    panic,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use goldeneye_domain::{FileRecord, GraphNode, NodeId};
use goldeneye_ports::QueryRepository;
use regex::Regex;

use super::{
    ClassifiedMatch, MAX_MATCH_LINES, MAX_RAW_OUTPUT, MAX_SEARCH_CODE_LIMIT, SLOW_SEARCH_MS,
    build_hit, compile_content_pattern, compile_file_pattern, compile_optional_pattern,
    directory_distribution, file_matches, tightest_node, valid_search_path_argument,
};
use crate::types::{
    QueryError, RawCodeMatch, SearchCodeFilesResult, SearchCodeMatchesResult, SearchCodeMode,
    SearchCodeRequest, SearchCodeResult,
};

const MAX_SCAN_WORKERS: usize = 8;
const MIN_FILES_PER_WORKER: usize = 64;

struct PreparedSearch<'a> {
    pattern: Regex,
    path_filter: Option<Regex>,
    file_pattern: Option<Regex>,
    root: PathBuf,
    canonical_root: PathBuf,
    node_degrees: &'a BTreeMap<NodeId, (usize, usize)>,
}

#[derive(Default)]
struct SearchScan {
    classified: BTreeMap<NodeId, ClassifiedMatch>,
    raw_matches: Vec<RawCodeMatch>,
    total_grep_matches: usize,
    sources: BTreeMap<String, String>,
}

pub(super) fn execute(
    repository: &dyn QueryRepository,
    request: &SearchCodeRequest,
    node_degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<SearchCodeResult, QueryError> {
    let started = Instant::now();
    validate_request(request)?;
    let prepared = prepare_search(repository, request, node_degrees)?;
    let scan = scan_repository(repository, request, &prepared)?;
    let SearchScan {
        classified,
        raw_matches,
        total_grep_matches,
        sources,
    } = scan;
    let matches = rank_matches(classified);
    if request.mode == SearchCodeMode::Files {
        return Ok(files_result(&matches, &raw_matches, request.limit));
    }
    Ok(matches_result(
        request,
        &matches,
        raw_matches,
        total_grep_matches,
        &sources,
        started.elapsed(),
    ))
}

pub(super) fn validate_request(request: &SearchCodeRequest) -> Result<(), QueryError> {
    if request.pattern.is_empty() {
        return Err(QueryError::EmptySearchPattern);
    }
    if request.limit == 0 || request.limit > MAX_SEARCH_CODE_LIMIT {
        return Err(QueryError::InvalidSearchCodeLimit {
            actual: request.limit,
            maximum: MAX_SEARCH_CODE_LIMIT,
        });
    }
    Ok(())
}

fn prepare_search<'a>(
    repository: &dyn QueryRepository,
    request: &SearchCodeRequest,
    node_degrees: &'a BTreeMap<NodeId, (usize, usize)>,
) -> Result<PreparedSearch<'a>, QueryError> {
    let project = repository
        .get_project(&request.project)?
        .ok_or_else(|| QueryError::ProjectNotFound(request.project.clone()))?;
    if !valid_search_path_argument(&project.root_path)
        || request
            .file_pattern
            .as_deref()
            .is_some_and(|pattern| !valid_search_path_argument(pattern))
    {
        return Err(QueryError::InvalidSearchPathArgument);
    }
    let pattern = compile_content_pattern(&request.pattern, request.regex)?;
    let path_filter = compile_optional_pattern("path_filter", request.path_filter.as_deref())?;
    let file_pattern = compile_file_pattern(request.file_pattern.as_deref())?;
    let root = PathBuf::from(&project.root_path);
    let canonical_root = fs::canonicalize(&root).map_err(|source| QueryError::SourceRead {
        path: root.clone(),
        source,
    })?;
    Ok(PreparedSearch {
        pattern,
        path_filter,
        file_pattern,
        root,
        canonical_root,
        node_degrees,
    })
}

fn scan_repository(
    repository: &dyn QueryRepository,
    request: &SearchCodeRequest,
    prepared: &PreparedSearch<'_>,
) -> Result<SearchScan, QueryError> {
    let files = repository.list_files(&request.project)?;
    let selected = files
        .iter()
        .filter(|file| file_selected(prepared, file.id.path.as_str()))
        .collect::<Vec<_>>();

    // Reading and matching is the bulk of the work and touches no shared
    // state, so it fans out. Graph lookups stay on this thread, in file
    // order, and only for the few files that actually contain a match.
    let mut scan = SearchScan::default();
    for (file, matched) in selected.iter().zip(grep_files(prepared, &selected)) {
        let Some(source) = matched? else {
            continue;
        };
        let relative = file.id.path.as_str();
        let file_nodes = repository.nodes_for_file(&file.id)?;
        classify_source(relative, &source, &file_nodes, prepared, &mut scan);
        scan.sources.insert(relative.to_owned(), source);
    }
    Ok(scan)
}

type GrepOutcome = Result<Option<String>, QueryError>;

fn grep_files(prepared: &PreparedSearch<'_>, files: &[&FileRecord]) -> Vec<GrepOutcome> {
    let grep_all = |part: &[&FileRecord]| {
        part.iter()
            .map(|file| grep_file(prepared, file.id.path.as_str()))
            .collect::<Vec<_>>()
    };
    let workers = thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(MAX_SCAN_WORKERS)
        .min(files.len() / MIN_FILES_PER_WORKER);
    if workers <= 1 {
        return grep_all(files);
    }

    let chunk_size = files.len().div_ceil(workers);
    thread::scope(|scope| {
        let handles = files
            .chunks(chunk_size)
            .map(|part| scope.spawn(move || grep_all(part)))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|payload| panic::resume_unwind(payload))
            })
            .collect()
    })
}

/// Returns the file's source when at least one of its lines matches.
///
/// A file that disappeared after indexing is skipped rather than failing the
/// whole search: the index is allowed to lag behind the working tree.
fn grep_file(prepared: &PreparedSearch<'_>, relative: &str) -> GrepOutcome {
    let absolute = prepared.root.join(relative);
    let bytes = match fs::read(&absolute) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(QueryError::SourceRead {
                path: absolute,
                source,
            });
        }
    };
    let source = String::from_utf8_lossy(&bytes).into_owned();
    if !source.lines().any(|line| prepared.pattern.is_match(line)) {
        return Ok(None);
    }

    // Containment is verified before any match is reported, so a link that
    // escapes the project can never surface content or even match counts.
    let canonical = fs::canonicalize(&absolute).map_err(|source| QueryError::SourceRead {
        path: absolute,
        source,
    })?;
    if !canonical.starts_with(&prepared.canonical_root) {
        return Err(QueryError::SourceOutsideProject { path: canonical });
    }
    Ok(Some(source))
}

fn file_selected(prepared: &PreparedSearch<'_>, relative: &str) -> bool {
    prepared
        .path_filter
        .as_ref()
        .is_none_or(|filter| filter.is_match(relative))
        && prepared
            .file_pattern
            .as_ref()
            .is_none_or(|filter| file_matches(filter, relative))
}

fn classify_source(
    relative: &str,
    source: &str,
    file_nodes: &[GraphNode],
    prepared: &PreparedSearch<'_>,
    scan: &mut SearchScan,
) {
    for (line_index, line) in source.lines().enumerate() {
        if !prepared.pattern.is_match(line) {
            continue;
        }
        scan.total_grep_matches += 1;
        let line_number = u64::try_from(line_index).unwrap_or(u64::MAX - 1) + 1;
        record_match(relative, line, line_number, file_nodes, prepared, scan);
    }
}

fn record_match(
    relative: &str,
    line: &str,
    line_number: u64,
    file_nodes: &[GraphNode],
    prepared: &PreparedSearch<'_>,
    scan: &mut SearchScan,
) {
    if let Some(node) = tightest_node(file_nodes, line_number) {
        let entry = scan
            .classified
            .entry(node.id.clone())
            .or_insert_with(|| ClassifiedMatch::new(node.clone(), prepared.node_degrees));
        if entry.match_lines.len() < MAX_MATCH_LINES {
            entry.match_lines.push(line_number);
        }
    } else {
        scan.raw_matches.push(RawCodeMatch {
            file: relative.to_owned(),
            line: line_number,
            content: line.trim_end_matches('\r').to_owned(),
        });
    }
}

fn rank_matches(classified: BTreeMap<NodeId, ClassifiedMatch>) -> Vec<ClassifiedMatch> {
    let mut matches = classified.into_values().collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.node.qualified_name.cmp(&right.node.qualified_name))
            .then_with(|| left.node.id.cmp(&right.node.id))
    });
    matches
}

fn files_result(
    matches: &[ClassifiedMatch],
    raw_matches: &[RawCodeMatch],
    limit: usize,
) -> SearchCodeResult {
    let mut files = BTreeSet::new();
    for result in matches.iter().take(limit) {
        if let Some(path) = &result.node.file_path {
            files.insert(path.as_str().to_owned());
        }
    }
    files.extend(raw_matches.iter().map(|result| result.file.clone()));
    SearchCodeResult::Files(SearchCodeFilesResult {
        files: files.into_iter().collect(),
    })
}

fn matches_result(
    request: &SearchCodeRequest,
    matches: &[ClassifiedMatch],
    mut raw_matches: Vec<RawCodeMatch>,
    total_grep_matches: usize,
    sources: &BTreeMap<String, String>,
    elapsed: Duration,
) -> SearchCodeResult {
    let total_results = matches.len();
    let directories = directory_distribution(matches);
    let results = matches
        .iter()
        .take(request.limit)
        .map(|result| build_hit(result, request, sources))
        .collect();
    let raw_match_count = raw_matches.len();
    raw_matches.truncate(MAX_RAW_OUTPUT);
    let elapsed_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    SearchCodeResult::Matches(SearchCodeMatchesResult {
        results,
        raw_matches,
        directories,
        total_grep_matches,
        total_results,
        raw_match_count,
        elapsed_ms,
        dedup_ratio: dedup_ratio(total_results, total_grep_matches, raw_match_count),
        warnings: search_warnings(request, elapsed_ms),
    })
}

fn dedup_ratio(
    total_results: usize,
    total_grep_matches: usize,
    raw_match_count: usize,
) -> Option<String> {
    (total_results > 0 && total_grep_matches > 0).then(|| {
        #[allow(clippy::cast_precision_loss)]
        let ratio = total_grep_matches as f64 / (total_results + raw_match_count) as f64;
        format!("{ratio:.1}x")
    })
}

fn search_warnings(request: &SearchCodeRequest, elapsed_ms: u64) -> Vec<String> {
    let mut warnings = Vec::new();
    if !request.regex && request.pattern.contains('|') {
        warnings.push(
            "pattern contains '|' but regex=false, so it is matched literally (not as \
             alternation). Pass regex=true for 'foo|bar' to mean 'foo OR bar'."
                .to_owned(),
        );
    }
    if elapsed_ms >= SLOW_SEARCH_MS {
        warnings.push(format!(
            "search took {elapsed_ms}ms (>5s); narrow file_pattern/path_filter or use a more \
             specific pattern"
        ));
    }
    warnings
}
