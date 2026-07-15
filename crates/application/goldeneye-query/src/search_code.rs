mod pipeline;

use std::{collections::BTreeMap, path::Path};

use goldeneye_domain::{GraphNode, NodeId};
use goldeneye_ports::QueryRepository;
use regex::Regex;

use crate::types::{
    QueryError, SearchCodeHit, SearchCodeMode, SearchCodeRequest, SearchCodeResult,
};

const MAX_SEARCH_CODE_LIMIT: usize = 200;
const MAX_MATCH_LINES: usize = 64;
const MAX_RAW_OUTPUT: usize = 20;
const FULL_SOURCE_LINES: u64 = 60;
const FULL_SOURCE_LEAD: u64 = 5;
const SLOW_SEARCH_MS: u64 = 5_000;

pub(crate) fn execute(
    repository: &dyn QueryRepository,
    request: &SearchCodeRequest,
) -> Result<SearchCodeResult, QueryError> {
    pipeline::execute(repository, request)
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
    let mut hit = base_hit(result);
    let Some(source) = sources.get(&hit.file) else {
        return hit;
    };
    if request.mode == SearchCodeMode::Full {
        attach_full_source(&mut hit, result, source);
    } else if request.context > 0 {
        attach_context(&mut hit, result, request.context, source);
    }
    hit
}

fn base_hit(result: &ClassifiedMatch) -> SearchCodeHit {
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
    SearchCodeHit {
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
    }
}

fn attach_full_source(hit: &mut SearchCodeHit, result: &ClassifiedMatch, source: &str) {
    let mut source_start = hit.start_line;
    let mut source_end = hit.end_line;
    let truncated = hit.end_line.saturating_sub(hit.start_line) + 1 > FULL_SOURCE_LINES;
    if truncated {
        if let Some(first_match) = result.match_lines.first()
            && first_match.saturating_sub(FULL_SOURCE_LEAD) > hit.start_line
        {
            source_start = first_match - FULL_SOURCE_LEAD;
        }
        source_end = (source_start + FULL_SOURCE_LINES - 1).min(hit.end_line);
    }
    hit.source = Some(read_lines(source, source_start, source_end));
    if truncated {
        hit.source_start = Some(source_start);
        hit.source_truncated = Some(true);
    }
}

fn attach_context(hit: &mut SearchCodeHit, result: &ClassifiedMatch, context: usize, source: &str) {
    if let (Some(first), Some(last)) = (result.match_lines.first(), result.match_lines.last()) {
        let context = u64::try_from(context).unwrap_or(u64::MAX);
        let context_start = first.saturating_sub(context).max(1);
        let context_end = last.saturating_add(context);
        hit.context = Some(read_lines(source, context_start, context_end));
        hit.context_start = Some(context_start);
    }
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
            .split([' ', '\t'])
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
