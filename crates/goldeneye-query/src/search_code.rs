use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

use goldeneye_domain::{GraphNode, NodeId};
use goldeneye_store::QueryStore;
use regex::Regex;

use crate::{
    engine::degrees,
    types::{
        QueryError, RawCodeMatch, SearchCodeFilesResult, SearchCodeHit, SearchCodeMatchesResult,
        SearchCodeMode, SearchCodeRequest, SearchCodeResult,
    },
};

const MAX_SEARCH_CODE_LIMIT: usize = 200;
const MAX_MATCH_LINES: usize = 64;
const MAX_RAW_OUTPUT: usize = 20;
const FULL_SOURCE_LINES: u64 = 60;
const FULL_SOURCE_LEAD: u64 = 5;
const SLOW_SEARCH_MS: u64 = 5_000;

pub(crate) fn execute(
    store: &QueryStore,
    request: &SearchCodeRequest,
) -> Result<SearchCodeResult, QueryError> {
    let started = Instant::now();
    if request.pattern.is_empty() {
        return Err(QueryError::EmptySearchPattern);
    }
    if request.limit == 0 || request.limit > MAX_SEARCH_CODE_LIMIT {
        return Err(QueryError::InvalidSearchCodeLimit {
            actual: request.limit,
            maximum: MAX_SEARCH_CODE_LIMIT,
        });
    }
    let project = store
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
    let edges = store.list_edges(&request.project)?;
    let node_degrees = degrees(&edges);
    let mut classified = BTreeMap::<NodeId, ClassifiedMatch>::new();
    let mut raw_matches = Vec::new();
    let mut total_grep_matches = 0_usize;
    let mut sources = BTreeMap::<String, String>::new();

    for file in store.list_files(&request.project)? {
        let relative = file.id.path.as_str();
        if path_filter
            .as_ref()
            .is_some_and(|filter| !filter.is_match(relative))
            || file_pattern
                .as_ref()
                .is_some_and(|filter| !file_matches(filter, relative))
        {
            continue;
        }
        let absolute = root.join(relative);
        let canonical = fs::canonicalize(&absolute).map_err(|source| QueryError::SourceRead {
            path: absolute.clone(),
            source,
        })?;
        if !canonical.starts_with(&canonical_root) {
            return Err(QueryError::SourceOutsideProject { path: canonical });
        }
        let bytes = fs::read(&canonical).map_err(|source| QueryError::SourceRead {
            path: canonical,
            source,
        })?;
        let source = String::from_utf8_lossy(&bytes).into_owned();
        let file_nodes = store.nodes_for_file(&file.id)?;
        for (line_index, line) in source.lines().enumerate() {
            if !pattern.is_match(line) {
                continue;
            }
            total_grep_matches += 1;
            let line_number = u64::try_from(line_index).unwrap_or(u64::MAX - 1) + 1;
            if let Some(node) = tightest_node(&file_nodes, line_number) {
                let entry = classified
                    .entry(node.id.clone())
                    .or_insert_with(|| ClassifiedMatch::new(node.clone(), &node_degrees));
                if entry.match_lines.len() < MAX_MATCH_LINES {
                    entry.match_lines.push(line_number);
                }
            } else {
                raw_matches.push(RawCodeMatch {
                    file: relative.to_owned(),
                    line: line_number,
                    content: line.trim_end_matches('\r').to_owned(),
                });
            }
        }
        sources.insert(relative.to_owned(), source);
    }

    let mut matches = classified.into_values().collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.node.qualified_name.cmp(&right.node.qualified_name))
            .then_with(|| left.node.id.cmp(&right.node.id))
    });
    let total_results = matches.len();

    if request.mode == SearchCodeMode::Files {
        let mut files = BTreeSet::new();
        for result in matches.iter().take(request.limit) {
            if let Some(path) = &result.node.file_path {
                files.insert(path.as_str().to_owned());
            }
        }
        files.extend(raw_matches.iter().map(|result| result.file.clone()));
        return Ok(SearchCodeResult::Files(SearchCodeFilesResult {
            files: files.into_iter().collect(),
        }));
    }

    let directories = directory_distribution(&matches);
    let results = matches
        .iter()
        .take(request.limit)
        .map(|result| build_hit(result, request, &sources))
        .collect();
    let raw_match_count = raw_matches.len();
    raw_matches.truncate(MAX_RAW_OUTPUT);
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let dedup_ratio = (total_results > 0 && total_grep_matches > 0).then(|| {
        #[allow(clippy::cast_precision_loss)]
        let ratio = total_grep_matches as f64 / (total_results + raw_match_count) as f64;
        format!("{ratio:.1}x")
    });
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

    Ok(SearchCodeResult::Matches(SearchCodeMatchesResult {
        results,
        raw_matches,
        directories,
        total_grep_matches,
        total_results,
        raw_match_count,
        elapsed_ms,
        dedup_ratio,
        warnings,
    }))
}

struct ClassifiedMatch {
    node: GraphNode,
    in_degree: usize,
    out_degree: usize,
    score: i64,
    match_lines: Vec<u64>,
}

impl ClassifiedMatch {
    fn new(node: GraphNode, node_degrees: &BTreeMap<NodeId, (usize, usize)>) -> Self {
        let (in_degree, out_degree) = node_degrees.get(&node.id).copied().unwrap_or((0, 0));
        let score = search_score(&node, in_degree);
        Self {
            node,
            in_degree,
            out_degree,
            score,
            match_lines: Vec::new(),
        }
    }
}

fn search_score(node: &GraphNode, in_degree: usize) -> i64 {
    let mut score = i64::try_from(in_degree).unwrap_or(i64::MAX);
    if matches!(node.label.as_str(), "Function" | "Method") {
        score = score.saturating_add(10);
    }
    if node.label.as_str() == "Route" {
        score = score.saturating_add(15);
    }
    if let Some(path) = &node.file_path {
        let path = path.as_str();
        if path.contains("vendored/") || path.contains("vendor/") || path.contains("node_modules/")
        {
            score = score.saturating_sub(50);
        }
        if path.contains("test") || path.contains("spec") || path.contains("_test.") {
            score = score.saturating_sub(5);
        }
    }
    score
}

fn tightest_node(nodes: &[GraphNode], line: u64) -> Option<&GraphNode> {
    nodes
        .iter()
        .filter(|node| {
            node.source_span.is_some_and(|span| {
                let start = span.start.row + 1;
                let end = span.end.row + 1;
                start <= line && end >= line
            })
        })
        .min_by(|left, right| {
            let left_span = left.source_span.expect("filtered span");
            let right_span = right.source_span.expect("filtered span");
            (left_span.end.row - left_span.start.row)
                .cmp(&(right_span.end.row - right_span.start.row))
                .then_with(|| left.id.cmp(&right.id))
        })
}

fn build_hit(
    result: &ClassifiedMatch,
    request: &SearchCodeRequest,
    sources: &BTreeMap<String, String>,
) -> SearchCodeHit {
    let span = result.node.source_span.expect("classified node has a span");
    let start_line = span.start.row + 1;
    let end_line = span.end.row + 1;
    let file = result
        .node
        .file_path
        .as_ref()
        .expect("classified node has a file")
        .as_str()
        .to_owned();
    let mut hit = SearchCodeHit {
        node: result.node.name.clone(),
        qualified_name: result.node.qualified_name.as_str().to_owned(),
        label: result.node.label.as_str().to_owned(),
        file: file.clone(),
        start_line,
        end_line,
        in_degree: result.in_degree,
        out_degree: result.out_degree,
        match_lines: result.match_lines.clone(),
        source: None,
        source_start: None,
        source_truncated: None,
        context: None,
        context_start: None,
    };
    let Some(source) = sources.get(&file) else {
        return hit;
    };
    if request.mode == SearchCodeMode::Full {
        let mut source_start = start_line;
        let mut source_end = end_line;
        let truncated = end_line.saturating_sub(start_line) + 1 > FULL_SOURCE_LINES;
        if truncated {
            if let Some(first_match) = result.match_lines.first()
                && first_match.saturating_sub(FULL_SOURCE_LEAD) > start_line
            {
                source_start = first_match - FULL_SOURCE_LEAD;
            }
            source_end = (source_start + FULL_SOURCE_LINES - 1).min(end_line);
        }
        hit.source = Some(read_lines(source, source_start, source_end));
        if truncated {
            hit.source_start = Some(source_start);
            hit.source_truncated = Some(true);
        }
    } else if request.context > 0
        && let (Some(first), Some(last)) = (result.match_lines.first(), result.match_lines.last())
    {
        let context = u64::try_from(request.context).unwrap_or(u64::MAX);
        let context_start = first.saturating_sub(context).max(1);
        let context_end = last.saturating_add(context);
        hit.context = Some(read_lines(source, context_start, context_end));
        hit.context_start = Some(context_start);
    }
    hit
}

fn read_lines(source: &str, start: u64, end: u64) -> String {
    source
        .split_inclusive('\n')
        .enumerate()
        .filter_map(|(index, line)| {
            let number = u64::try_from(index).unwrap_or(u64::MAX - 1) + 1;
            (number >= start && number <= end).then_some(line)
        })
        .collect()
}

fn directory_distribution(matches: &[ClassifiedMatch]) -> BTreeMap<String, usize> {
    let mut directories = BTreeMap::new();
    for result in matches {
        let Some(path) = &result.node.file_path else {
            continue;
        };
        let path = path.as_str();
        let directory = path
            .split_once('/')
            .map_or_else(|| path.to_owned(), |(head, _)| format!("{head}/"));
        *directories.entry(directory).or_default() += 1;
    }
    directories
}

fn compile_content_pattern(pattern: &str, use_regex: bool) -> Result<Regex, QueryError> {
    let compiled = if use_regex {
        pattern.to_owned()
    } else if pattern.bytes().any(|byte| matches!(byte, b' ' | b'\t')) {
        pattern
            .split(|character| matches!(character, ' ' | '\t'))
            .filter(|word| !word.is_empty())
            .map(regex::escape)
            .collect::<Vec<_>>()
            .join(".*")
    } else {
        regex::escape(pattern)
    };
    Regex::new(&compiled).map_err(|source| QueryError::InvalidPattern {
        field: "pattern",
        source,
    })
}

fn compile_optional_pattern(
    field: &'static str,
    pattern: Option<&str>,
) -> Result<Option<Regex>, QueryError> {
    pattern
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| {
            Regex::new(pattern).map_err(|source| QueryError::InvalidPattern { field, source })
        })
        .transpose()
}

fn compile_file_pattern(pattern: Option<&str>) -> Result<Option<Regex>, QueryError> {
    pattern
        .map(|pattern| {
            let escaped = regex::escape(pattern)
                .replace(r"\*", ".*")
                .replace(r"\?", ".");
            Regex::new(&format!("^{escaped}$")).map_err(|source| QueryError::InvalidPattern {
                field: "file_pattern",
                source,
            })
        })
        .transpose()
}

fn file_matches(pattern: &Regex, relative: &str) -> bool {
    pattern.is_match(relative)
        || Path::new(relative)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| pattern.is_match(name))
}

fn valid_search_path_argument(value: &str) -> bool {
    !value.chars().any(|character| {
        matches!(
            character,
            '\'' | '"' | ';' | '|' | '$' | '`' | '<' | '>' | '\n' | '\r'
        ) || (!cfg!(windows) && character == '\\')
    })
}
