use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
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
use crate::{
    engine::degrees,
    types::{
        QueryError, RawCodeMatch, SearchCodeFilesResult, SearchCodeMatchesResult, SearchCodeMode,
        SearchCodeRequest, SearchCodeResult,
    },
};

struct PreparedSearch {
    pattern: Regex,
    path_filter: Option<Regex>,
    file_pattern: Option<Regex>,
    root: PathBuf,
    canonical_root: PathBuf,
    node_degrees: BTreeMap<NodeId, (usize, usize)>,
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
) -> Result<SearchCodeResult, QueryError> {
    let started = Instant::now();
    validate_request(request)?;
    let prepared = prepare_search(repository, request)?;
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

fn validate_request(request: &SearchCodeRequest) -> Result<(), QueryError> {
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

fn prepare_search(
    repository: &dyn QueryRepository,
    request: &SearchCodeRequest,
) -> Result<PreparedSearch, QueryError> {
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
    let node_degrees = degrees(&repository.list_edges(&request.project)?);
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
    prepared: &PreparedSearch,
) -> Result<SearchScan, QueryError> {
    let mut scan = SearchScan::default();
    for file in repository.list_files(&request.project)? {
        scan_file(repository, prepared, &file, &mut scan)?;
    }
    Ok(scan)
}

fn scan_file(
    repository: &dyn QueryRepository,
    prepared: &PreparedSearch,
    file: &FileRecord,
    scan: &mut SearchScan,
) -> Result<(), QueryError> {
    let relative = file.id.path.as_str();
    if !file_selected(prepared, relative) {
        return Ok(());
    }
    let source = read_project_source(prepared, relative)?;
    let file_nodes = repository.nodes_for_file(&file.id)?;
    classify_source(relative, &source, &file_nodes, prepared, scan);
    scan.sources.insert(relative.to_owned(), source);
    Ok(())
}

fn file_selected(prepared: &PreparedSearch, relative: &str) -> bool {
    prepared
        .path_filter
        .as_ref()
        .is_none_or(|filter| filter.is_match(relative))
        && prepared
            .file_pattern
            .as_ref()
            .is_none_or(|filter| file_matches(filter, relative))
}

fn read_project_source(prepared: &PreparedSearch, relative: &str) -> Result<String, QueryError> {
    let absolute = prepared.root.join(relative);
    let canonical = fs::canonicalize(&absolute).map_err(|source| QueryError::SourceRead {
        path: absolute,
        source,
    })?;
    if !canonical.starts_with(&prepared.canonical_root) {
        return Err(QueryError::SourceOutsideProject { path: canonical });
    }
    let bytes = fs::read(&canonical).map_err(|source| QueryError::SourceRead {
        path: canonical,
        source,
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn classify_source(
    relative: &str,
    source: &str,
    file_nodes: &[GraphNode],
    prepared: &PreparedSearch,
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
    prepared: &PreparedSearch,
    scan: &mut SearchScan,
) {
    if let Some(node) = tightest_node(file_nodes, line_number) {
        let entry = scan
            .classified
            .entry(node.id.clone())
            .or_insert_with(|| ClassifiedMatch::new(node.clone(), &prepared.node_degrees));
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
