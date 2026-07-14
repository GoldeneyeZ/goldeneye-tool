// Cypher coercion deliberately mirrors upstream JavaScript/SQLite numeric semantics; the
// parser/evaluator conformance suite covers these bounded conversions and state machines.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    clippy::type_complexity
)]

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};
use regex::Regex;

use crate::{
    engine::node_summary,
    types::{EdgeSummary, QueryError, QueryGraphRequest, QueryGraphResult, QueryValue},
};

const MAX_QUERY_ROWS: usize = 10_000;
const MAX_QUERY_BYTES: usize = 1_048_576;
const MAX_QUERY_TOKENS: usize = 16_384;
const MAX_UNION_BRANCHES: usize = 32;
const MAX_MATCH_PATTERNS: usize = 64;
const MAX_PROJECTIONS: usize = 256;
const MAX_VARIABLE_HOPS: usize = 10;
const MAX_INTERMEDIATE_BINDINGS: usize = 100_000;

pub(crate) fn execute(
    request: &QueryGraphRequest,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryGraphResult, QueryError> {
    if request.max_rows == 0 || request.max_rows > MAX_QUERY_ROWS {
        return Err(QueryError::InvalidQueryRowLimit {
            actual: request.max_rows,
            maximum: MAX_QUERY_ROWS,
        });
    }
    if request.query.len() > MAX_QUERY_BYTES {
        return Err(unsupported("query exceeds byte-size safety cap"));
    }
    let tokens = lex(&request.query)?;
    if tokens.len() > MAX_QUERY_TOKENS {
        return Err(unsupported("query exceeds token-count safety cap"));
    }
    reject_mutations(&tokens)?;
    let (branches, union_all) = split_union_tokens(&tokens)?;
    if branches.len() > MAX_UNION_BRANCHES {
        return Err(unsupported("query exceeds UNION branch safety cap"));
    }
    if union_all.is_empty() {
        let query = Parser::new(
            branches.into_iter().next().expect("query has one branch"),
            request.query.len(),
        )
        .parse()?;
        return execute_parsed(request, query, nodes, edges, degrees, request.max_rows);
    }
    let mut results = branches
        .into_iter()
        .map(|tokens| {
            Parser::new(tokens, request.query.len())
                .parse()
                .and_then(|query| {
                    execute_parsed(request, query, nodes, edges, degrees, MAX_QUERY_ROWS)
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let first = results
        .first()
        .ok_or_else(|| unsupported("UNION requires at least one query"))?;
    let columns = first.columns.clone();
    if results.iter().any(|result| result.columns != columns) {
        return Err(unsupported("UNION branches must return identical columns"));
    }
    let first_result = results.remove(0);
    let mut source_truncated = first_result.truncated;
    let mut warnings = first_result.warning.into_iter().collect::<Vec<_>>();
    let mut rows = first_result.rows;
    for (all, result) in union_all.into_iter().zip(results) {
        source_truncated |= result.truncated;
        warnings.extend(result.warning);
        rows.extend(result.rows);
        if !all {
            let mut seen = BTreeSet::new();
            rows.retain(|row| seen.insert(row_key(row)));
        }
    }
    let total = rows.len();
    let truncated = source_truncated || total > request.max_rows;
    rows.truncate(request.max_rows);
    Ok(QueryGraphResult {
        project: request.project.as_str().to_owned(),
        columns,
        rows,
        total,
        truncated,
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
    })
}

fn execute_parsed(
    request: &QueryGraphRequest,
    mut query: ParsedQuery,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    row_cap: usize,
) -> Result<QueryGraphResult, QueryError> {
    if let Some(result) = execute_simple_node_limit(request, &query, nodes, edges, degrees, row_cap)
    {
        return result;
    }
    let initial = execute_unwind(query.unwind.as_ref(), degrees)?;
    let mut bindings = execute_match_clauses(&query.matches, initial, nodes, edges, degrees)?;
    if let Some(with_clause) = &query.with_clause {
        bindings = execute_with_clause(with_clause, bindings, degrees)?;
    }

    if query.star {
        query.projections = query.with_clause.as_ref().map_or_else(
            || expand_star_projections(&query.matches),
            expand_with_star_projections,
        );
    }
    let columns = query
        .projections
        .iter()
        .map(|projection| projection.column.clone())
        .collect();
    let query_limit = query.limit.unwrap_or(usize::MAX);
    let materialized_limit = row_cap.min(query_limit);
    let has_aggregate = query.projections.iter().any(|projection| {
        matches!(
            projection.expression,
            ProjectionExpression::Aggregate { .. }
        )
    });
    let (rows, total, truncated) = if !has_aggregate && !query.distinct && query.order.is_empty() {
        let projected = bindings.into_iter().map(|binding| {
            query
                .projections
                .iter()
                .map(|projection| {
                    evaluate_projection_expression(&projection.expression, &binding, degrees)
                })
                .collect::<Result<Vec<_>, _>>()
        });
        let bounded = collect_bounded_rows(projected, query.skip, materialized_limit)?;
        (bounded.rows, bounded.total, bounded.truncated)
    } else {
        let mut rows = materialize_rows(&query, bindings, degrees)?;
        if query.distinct {
            let mut seen = BTreeSet::new();
            rows.retain(|row| seen.insert(row_key(row)));
        }
        let skipped = query.skip.min(rows.len());
        rows.drain(..skipped);
        let total = rows.len();
        let truncated = total > materialized_limit;
        rows.truncate(materialized_limit);
        (rows, total, truncated)
    };
    let warning = (!query.warnings.is_empty()).then(|| query.warnings.join("; "));

    Ok(QueryGraphResult {
        project: request.project.as_str().to_owned(),
        columns,
        rows,
        total,
        truncated,
        warning,
    })
}

fn execute_simple_node_limit(
    request: &QueryGraphRequest,
    query: &ParsedQuery,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    row_cap: usize,
) -> Option<Result<QueryGraphResult, QueryError>> {
    let query_limit = query.limit?;
    let [clause] = query.matches.as_slice() else {
        return None;
    };
    let [MatchPattern::Node(pattern)] = clause.patterns.as_slice() else {
        return None;
    };
    if query.unwind.is_some()
        || query.with_clause.is_some()
        || clause.optional
        || query.distinct
        || query.star
        || !query.order.is_empty()
        || query.projections.iter().any(|projection| {
            matches!(
                projection.expression,
                ProjectionExpression::Aggregate { .. }
            )
        })
    {
        return None;
    }

    // Buffer only references so binding-cap errors retain priority over expression errors.
    let mut candidates = Vec::with_capacity(nodes.len().min(MAX_INTERMEDIATE_BINDINGS));
    for node in nodes {
        if node_matches(node, pattern, degrees) {
            if candidates.len() == MAX_INTERMEDIATE_BINDINGS {
                return Some(Err(unsupported(
                    "query exceeds intermediate binding safety cap",
                )));
            }
            candidates.push(node);
        }
    }

    if clause.filter.is_none()
        && let [
            Projection {
                expression: ProjectionExpression::Reference(Reference::Property { alias, path }),
                ..
            },
        ] = query.projections.as_slice()
        && alias == &pattern.alias
        && path.as_slice() == ["qualified_name"]
    {
        candidates.sort_by_cached_key(|node| {
            serde_json::to_string(node.qualified_name.as_str())
                .unwrap_or_else(|_| node.qualified_name.as_str().to_owned())
        });
        let skipped = query.skip.min(candidates.len());
        let total = candidates.len() - skipped;
        let limit = row_cap.min(query_limit);
        let rows = candidates
            .into_iter()
            .skip(skipped)
            .take(limit)
            .map(|node| vec![QueryValue::String(node.qualified_name.as_str().to_owned())])
            .collect();
        return Some(Ok(QueryGraphResult {
            project: request.project.as_str().to_owned(),
            columns: query
                .projections
                .iter()
                .map(|projection| projection.column.clone())
                .collect(),
            rows,
            total,
            truncated: total > limit,
            warning: (!query.warnings.is_empty()).then(|| query.warnings.join("; ")),
        }));
    }

    if clause.filter.is_none()
        && let [
            Projection {
                expression: ProjectionExpression::Reference(Reference::Alias(alias)),
                ..
            },
        ] = query.projections.as_slice()
        && alias == &pattern.alias
    {
        // QueryValue::Node row keys start with the unique node ID. Sorting its exact JSON
        // encoding therefore preserves collect_bounded_rows ordering without constructing a
        // full NodeSummary for every candidate.
        candidates.sort_by_cached_key(|node| {
            serde_json::to_string(node.id.as_str()).unwrap_or_else(|_| node.id.as_str().to_owned())
        });
        let skipped = query.skip.min(candidates.len());
        let total = candidates.len() - skipped;
        let limit = row_cap.min(query_limit);
        let rows = candidates
            .into_iter()
            .skip(skipped)
            .take(limit)
            .map(|node| {
                vec![QueryValue::Node(node_summary(
                    node,
                    None,
                    degrees,
                    Vec::new(),
                ))]
            })
            .collect();
        return Some(Ok(QueryGraphResult {
            project: request.project.as_str().to_owned(),
            columns: query
                .projections
                .iter()
                .map(|projection| projection.column.clone())
                .collect(),
            rows,
            total,
            truncated: total > limit,
            warning: (!query.warnings.is_empty()).then(|| query.warnings.join("; ")),
        }));
    }

    let projected = candidates.into_iter().filter_map(|node| {
        let binding = Binding {
            nodes: BTreeMap::from([(pattern.alias.clone(), node)]),
            edges: BTreeMap::new(),
            values: BTreeMap::new(),
            all_nodes: Some(nodes),
            all_edges: Some(edges),
        };
        if let Some(filter) = &clause.filter {
            match evaluate_expression(filter, &binding, degrees) {
                Ok(true) => {}
                Ok(false) => return None,
                Err(error) => return Some(Err(error)),
            }
        }
        Some(
            query
                .projections
                .iter()
                .map(|projection| {
                    evaluate_projection_expression(&projection.expression, &binding, degrees)
                })
                .collect::<Result<Vec<_>, _>>(),
        )
    });
    let bounded = match collect_bounded_rows(projected, query.skip, row_cap.min(query_limit)) {
        Ok(bounded) => bounded,
        Err(error) => return Some(Err(error)),
    };
    Some(Ok(QueryGraphResult {
        project: request.project.as_str().to_owned(),
        columns: query
            .projections
            .iter()
            .map(|projection| projection.column.clone())
            .collect(),
        rows: bounded.rows,
        total: bounded.total,
        truncated: bounded.truncated,
        warning: (!query.warnings.is_empty()).then(|| query.warnings.join("; ")),
    }))
}

#[derive(Debug, Clone, PartialEq)]
enum TokenKind {
    Identifier(String),
    String(String),
    Number(String),
    Symbol(Symbol),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Symbol {
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LeftBrace,
    RightBrace,
    Colon,
    Comma,
    Dot,
    Dash,
    ArrowRight,
    ArrowLeft,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Regex,
    Star,
    Pipe,
}

#[derive(Debug, Clone)]
struct Token {
    kind: TokenKind,
    position: usize,
}

fn lex(input: &str) -> Result<Vec<Token>, QueryError> {
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character.is_whitespace() {
            index += character.len_utf8();
            continue;
        }
        if character == '\'' || character == '"' {
            let (value, next) = lex_string(input, index, character)?;
            tokens.push(Token {
                kind: TokenKind::String(value),
                position: index,
            });
            index = next;
            continue;
        }
        if character == '`' {
            let (value, next) = lex_backtick_identifier(input, index)?;
            tokens.push(Token {
                kind: TokenKind::Identifier(value),
                position: index,
            });
            index = next;
            continue;
        }
        if character.is_ascii_digit() {
            let next = lex_number_end(input, index);
            tokens.push(Token {
                kind: TokenKind::Number(input[index..next].to_owned()),
                position: index,
            });
            index = next;
            continue;
        }
        if character == '_' || character.is_alphabetic() {
            let next = lex_identifier_end(input, index);
            tokens.push(Token {
                kind: TokenKind::Identifier(input[index..next].to_owned()),
                position: index,
            });
            index = next;
            continue;
        }
        let (symbol, consumed) = lex_symbol(input, index)?;
        tokens.push(Token {
            kind: TokenKind::Symbol(symbol),
            position: index,
        });
        index += consumed;
    }
    Ok(tokens)
}

fn lex_string(input: &str, start: usize, quote: char) -> Result<(String, usize), QueryError> {
    let mut value = String::new();
    let mut index = start + quote.len_utf8();
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character == quote {
            return Ok((value, index + character.len_utf8()));
        }
        if character == '\\' {
            index += character.len_utf8();
            let escaped = input[index..]
                .chars()
                .next()
                .ok_or_else(|| syntax(start, "unterminated string escape"))?;
            value.push(match escaped {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '\\' => '\\',
                '\'' => '\'',
                '"' => '"',
                other => other,
            });
            index += escaped.len_utf8();
            continue;
        }
        value.push(character);
        index += character.len_utf8();
    }
    Err(syntax(start, "unterminated string literal"))
}

fn lex_backtick_identifier(input: &str, start: usize) -> Result<(String, usize), QueryError> {
    let mut value = String::new();
    let mut index = start + 1;
    while index < input.len() {
        let character = input[index..]
            .chars()
            .next()
            .expect("index remains on a character boundary");
        if character == '`' {
            return Ok((value, index + 1));
        }
        value.push(character);
        index += character.len_utf8();
    }
    Err(syntax(start, "unterminated backtick identifier"))
}

fn lex_number_end(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    let mut index = start;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        index += 1;
    }
    if index + 1 < bytes.len() && bytes[index] == b'.' && bytes[index + 1].is_ascii_digit() {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }
    }
    index
}

fn lex_identifier_end(input: &str, start: usize) -> usize {
    let mut end = start;
    for (offset, character) in input[start..].char_indices() {
        if offset == 0 || character == '_' || character.is_alphanumeric() {
            end = start + offset + character.len_utf8();
        } else {
            break;
        }
    }
    end
}

fn lex_symbol(input: &str, index: usize) -> Result<(Symbol, usize), QueryError> {
    let rest = &input[index..];
    let pair = rest.get(..2);
    if let Some(symbol) = pair.and_then(|pair| match pair {
        "->" => Some(Symbol::ArrowRight),
        "<-" => Some(Symbol::ArrowLeft),
        "<>" | "!=" => Some(Symbol::NotEqual),
        "<=" => Some(Symbol::LessEqual),
        ">=" => Some(Symbol::GreaterEqual),
        "=~" => Some(Symbol::Regex),
        _ => None,
    }) {
        return Ok((symbol, 2));
    }
    let symbol = match rest.as_bytes()[0] {
        b'(' => Symbol::LeftParen,
        b')' => Symbol::RightParen,
        b'[' => Symbol::LeftBracket,
        b']' => Symbol::RightBracket,
        b'{' => Symbol::LeftBrace,
        b'}' => Symbol::RightBrace,
        b':' => Symbol::Colon,
        b',' => Symbol::Comma,
        b'.' => Symbol::Dot,
        b'-' => Symbol::Dash,
        b'=' => Symbol::Equal,
        b'<' => Symbol::Less,
        b'>' => Symbol::Greater,
        b'*' => Symbol::Star,
        b'|' => Symbol::Pipe,
        _ => return Err(syntax(index, "unsupported character")),
    };
    Ok((symbol, 1))
}

fn reject_mutations(tokens: &[Token]) -> Result<(), QueryError> {
    const MUTATING: &[&str] = &[
        "ALTER", "CALL", "CREATE", "DELETE", "DETACH", "DROP", "FOREACH", "INSERT", "LOAD",
        "MERGE", "REMOVE", "SET", "UPDATE",
    ];
    for token in tokens {
        let TokenKind::Identifier(identifier) = &token.kind else {
            continue;
        };
        if let Some(keyword) = MUTATING
            .iter()
            .find(|keyword| identifier.eq_ignore_ascii_case(keyword))
        {
            return Err(QueryError::MutatingQuery {
                keyword: (*keyword).to_owned(),
            });
        }
    }
    Ok(())
}

fn split_union_tokens(tokens: &[Token]) -> Result<(Vec<Vec<Token>>, Vec<bool>), QueryError> {
    let mut branches = Vec::new();
    let mut modes = Vec::new();
    let mut current = Vec::new();
    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    let mut braces = 0usize;
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index];
        let at_top_level = parentheses == 0 && brackets == 0 && braces == 0;
        if at_top_level
            && matches!(
                &token.kind,
                TokenKind::Identifier(identifier) if identifier.eq_ignore_ascii_case("UNION")
            )
        {
            if current.is_empty() {
                return Err(syntax(token.position, "UNION is missing its left query"));
            }
            branches.push(std::mem::take(&mut current));
            index += 1;
            let all = tokens.get(index).is_some_and(|token| {
                matches!(
                    &token.kind,
                    TokenKind::Identifier(identifier) if identifier.eq_ignore_ascii_case("ALL")
                )
            });
            if all {
                index += 1;
            }
            modes.push(all);
            continue;
        }
        if let TokenKind::Symbol(symbol) = token.kind {
            match symbol {
                Symbol::LeftParen => parentheses = parentheses.saturating_add(1),
                Symbol::RightParen => parentheses = parentheses.saturating_sub(1),
                Symbol::LeftBracket => brackets = brackets.saturating_add(1),
                Symbol::RightBracket => brackets = brackets.saturating_sub(1),
                Symbol::LeftBrace => braces = braces.saturating_add(1),
                Symbol::RightBrace => braces = braces.saturating_sub(1),
                _ => {}
            }
        }
        current.push(token.clone());
        index += 1;
    }
    if current.is_empty() {
        let position = tokens.last().map_or(0, |token| token.position);
        return Err(syntax(position, "UNION is missing its right query"));
    }
    branches.push(current);
    Ok((branches, modes))
}

#[derive(Debug)]
struct ParsedQuery {
    unwind: Option<UnwindClause>,
    matches: Vec<MatchClause>,
    with_clause: Option<WithClause>,
    distinct: bool,
    star: bool,
    projections: Vec<Projection>,
    order: Vec<OrderClause>,
    skip: usize,
    limit: Option<usize>,
    warnings: Vec<String>,
}

#[derive(Debug)]
struct UnwindClause {
    expression: Operand,
    alias: String,
}

#[derive(Debug)]
struct MatchClause {
    patterns: Vec<MatchPattern>,
    filter: Option<Expression>,
    optional: bool,
}

#[derive(Debug)]
struct WithClause {
    distinct: bool,
    projections: Vec<Projection>,
    filter: Option<Expression>,
    order: Vec<OrderClause>,
    skip: usize,
    limit: Option<usize>,
}

#[derive(Debug, Clone)]
enum MatchPattern {
    Node(NodePattern),
    Edge(Box<EdgeMatch>),
}

#[derive(Debug, Clone)]
struct EdgeMatch {
    left: NodePattern,
    edge: EdgePattern,
    right: NodePattern,
}

#[derive(Debug, Clone)]
struct NodePattern {
    alias: String,
    labels: Vec<String>,
    properties: Vec<(String, QueryValue)>,
}

#[derive(Debug, Clone)]
struct EdgePattern {
    alias: Option<String>,
    kinds: Vec<String>,
    direction: EdgeDirection,
    min_hops: usize,
    max_hops: usize,
}

#[derive(Debug, Clone, Copy)]
enum EdgeDirection {
    Outbound,
    Inbound,
    Undirected,
}

#[derive(Debug)]
enum Expression {
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Xor(Box<Self>, Box<Self>),
    Not(Box<Self>),
    Exists(Vec<MatchPattern>),
    Predicate(Box<Predicate>),
}

#[derive(Debug)]
struct Predicate {
    left: Operand,
    operator: PredicateOperator,
    right: Option<Operand>,
}

#[derive(Debug)]
enum PredicateOperator {
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Regex,
    In,
    NotIn,
    Contains,
    StartsWith,
    EndsWith,
    HasLabel(Vec<String>),
    IsNull,
    IsNotNull,
}

#[derive(Debug)]
enum Operand {
    Literal(Box<QueryValue>),
    List(Vec<Self>),
    Reference(Reference),
    Function { name: String, arguments: Vec<Self> },
}

#[derive(Debug, Clone)]
enum Reference {
    Alias(String),
    Property { alias: String, path: Vec<String> },
    EdgeType(String),
}

#[derive(Debug)]
struct Projection {
    expression: ProjectionExpression,
    column: String,
}

#[derive(Debug)]
enum ProjectionExpression {
    Reference(Reference),
    Function {
        name: String,
        arguments: Vec<Operand>,
    },
    Case(CaseExpression),
    Aggregate {
        kind: AggregateKind,
        target: Option<Reference>,
        distinct: bool,
    },
}

#[derive(Debug)]
struct CaseExpression {
    subject: Option<Operand>,
    branches: Vec<CaseBranch>,
    fallback: Option<Operand>,
}

#[derive(Debug)]
struct CaseBranch {
    when: CaseWhen,
    then: Operand,
}

#[derive(Debug)]
enum CaseWhen {
    Predicate(Expression),
    Value(Operand),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AggregateKind {
    Count,
    Sum,
    Average,
    Minimum,
    Maximum,
    Collect,
}

#[derive(Debug)]
struct OrderClause {
    reference: Reference,
    descending: bool,
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    end_position: usize,
    anonymous_nodes: usize,
    warnings: Vec<String>,
}

impl Parser {
    const fn new(tokens: Vec<Token>, end_position: usize) -> Self {
        Self {
            tokens,
            index: 0,
            end_position,
            anonymous_nodes: 0,
            warnings: Vec::new(),
        }
    }

    fn parse(mut self) -> Result<ParsedQuery, QueryError> {
        let unwind = if self.consume_keyword("UNWIND") {
            let expression = self.parse_operand()?;
            self.expect_keyword("AS")?;
            let alias = self.parse_identifier("UNWIND alias")?;
            Some(UnwindClause { expression, alias })
        } else {
            None
        };
        self.expect_keyword("MATCH")?;
        let matches = self.parse_match_clauses()?;
        let with_clause = if self.consume_keyword("WITH") {
            Some(self.parse_with_clause()?)
        } else {
            None
        };
        self.expect_keyword("RETURN")?;
        let distinct = self.consume_keyword("DISTINCT");
        let star = self.consume_symbol(Symbol::Star);
        let projections = if star {
            Vec::new()
        } else {
            self.parse_projections()?
        };
        let order = if self.consume_keyword("ORDER") {
            self.expect_keyword("BY")?;
            self.parse_order_clauses()?
        } else {
            Vec::new()
        };
        let mut skip = 0;
        let mut limit = None;
        loop {
            if self.consume_keyword("SKIP") {
                if skip != 0 {
                    return Err(self.error("duplicate SKIP clause"));
                }
                skip = self.parse_usize("SKIP")?;
            } else if self.consume_keyword("LIMIT") {
                if limit.is_some() {
                    return Err(self.error("duplicate LIMIT clause"));
                }
                limit = Some(self.parse_usize("LIMIT")?);
            } else {
                break;
            }
        }
        if self.peek().is_some() {
            return Err(self.error("unsupported trailing clause"));
        }
        Ok(ParsedQuery {
            unwind,
            matches,
            with_clause,
            distinct,
            star,
            projections,
            order,
            skip,
            limit,
            warnings: self.warnings,
        })
    }

    fn parse_match_clauses(&mut self) -> Result<Vec<MatchClause>, QueryError> {
        let mut clauses = Vec::new();
        let mut optional = false;
        loop {
            let mut patterns = self.parse_pattern_chain()?;
            while self.consume_symbol(Symbol::Comma) {
                patterns.extend(self.parse_pattern_chain()?);
            }
            let filter = if self.consume_keyword("WHERE") {
                Some(self.parse_or_expression()?)
            } else {
                None
            };
            clauses.push(MatchClause {
                patterns,
                filter,
                optional,
            });
            if clauses
                .iter()
                .map(|clause| clause.patterns.len())
                .sum::<usize>()
                > MAX_MATCH_PATTERNS
            {
                return Err(unsupported("query exceeds MATCH pattern safety cap"));
            }
            if self.consume_keyword("OPTIONAL") {
                self.expect_keyword("MATCH")?;
                optional = true;
            } else if self.consume_keyword("MATCH") {
                optional = false;
            } else {
                break;
            }
        }
        Ok(clauses)
    }

    fn parse_with_clause(&mut self) -> Result<WithClause, QueryError> {
        let distinct = self.consume_keyword("DISTINCT");
        let projections = self.parse_projections()?;
        let mut filter = None;
        let mut order = Vec::new();
        let mut skip = 0;
        let mut limit = None;
        loop {
            if self.consume_keyword("WHERE") {
                if filter.is_some() {
                    return Err(self.error("duplicate WHERE after WITH"));
                }
                filter = Some(self.parse_or_expression()?);
            } else if self.consume_keyword("ORDER") {
                if !order.is_empty() {
                    return Err(self.error("duplicate ORDER BY after WITH"));
                }
                self.expect_keyword("BY")?;
                order = self.parse_order_clauses()?;
            } else if self.consume_keyword("SKIP") {
                if skip != 0 {
                    return Err(self.error("duplicate SKIP after WITH"));
                }
                skip = self.parse_usize("SKIP")?;
            } else if self.consume_keyword("LIMIT") {
                if limit.is_some() {
                    return Err(self.error("duplicate LIMIT after WITH"));
                }
                limit = Some(self.parse_usize("LIMIT")?);
            } else {
                break;
            }
        }
        Ok(WithClause {
            distinct,
            projections,
            filter,
            order,
            skip,
            limit,
        })
    }

    fn parse_pattern_chain(&mut self) -> Result<Vec<MatchPattern>, QueryError> {
        let mut left = self.parse_node_pattern()?;
        let mut patterns = Vec::new();
        loop {
            let Some((edge, right)) = self.parse_pattern_relationship()? else {
                break;
            };
            if edge
                .alias
                .as_deref()
                .is_some_and(|alias| alias == left.alias || alias == right.alias)
            {
                return Err(unsupported("aliases in a relationship must be distinct"));
            }
            patterns.push(MatchPattern::Edge(Box::new(EdgeMatch {
                left: left.clone(),
                edge,
                right: right.clone(),
            })));
            left = right;
        }
        if patterns.is_empty() {
            patterns.push(MatchPattern::Node(left));
        }
        Ok(patterns)
    }

    fn parse_pattern_relationship(
        &mut self,
    ) -> Result<Option<(EdgePattern, NodePattern)>, QueryError> {
        let (edge, right) = if self.consume_symbol(Symbol::Dash) {
            let mut edge = self.parse_edge_pattern()?;
            edge.direction = if self.consume_symbol(Symbol::ArrowRight) {
                EdgeDirection::Outbound
            } else if self.consume_symbol(Symbol::Dash) {
                EdgeDirection::Undirected
            } else {
                return Err(self.error("expected -> or - after relationship"));
            };
            (edge, self.parse_node_pattern()?)
        } else if self.consume_symbol(Symbol::ArrowLeft) {
            let mut edge = self.parse_edge_pattern()?;
            self.expect_symbol(Symbol::Dash)?;
            edge.direction = EdgeDirection::Inbound;
            (edge, self.parse_node_pattern()?)
        } else {
            return Ok(None);
        };
        Ok(Some((edge, right)))
    }

    fn parse_node_pattern(&mut self) -> Result<NodePattern, QueryError> {
        self.expect_symbol(Symbol::LeftParen)?;
        let alias = if matches!(self.peek(), Some(TokenKind::Identifier(_))) {
            self.parse_identifier("node alias")?
        } else {
            let alias = format!("__anon_node_{}", self.anonymous_nodes);
            self.anonymous_nodes = self.anonymous_nodes.saturating_add(1);
            alias
        };
        let mut labels = Vec::new();
        if self.consume_symbol(Symbol::Colon) {
            labels.push(self.parse_identifier("node label")?);
            while self.consume_symbol(Symbol::Pipe) {
                self.consume_symbol(Symbol::Colon);
                labels.push(self.parse_identifier("node label")?);
            }
        }
        let mut properties = Vec::new();
        if self.consume_symbol(Symbol::LeftBrace) && !self.consume_symbol(Symbol::RightBrace) {
            loop {
                let key = self.parse_identifier("inline property name")?;
                self.expect_symbol(Symbol::Colon)?;
                let Operand::Literal(value) = self.parse_operand()? else {
                    return Err(self.error("inline properties require literal values"));
                };
                properties.push((key, *value));
                if !self.consume_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RightBrace)?;
        }
        self.expect_symbol(Symbol::RightParen)?;
        Ok(NodePattern {
            alias,
            labels,
            properties,
        })
    }

    fn parse_edge_pattern(&mut self) -> Result<EdgePattern, QueryError> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut alias = None;
        let mut kinds = Vec::new();
        if self.consume_symbol(Symbol::Colon) {
            kinds.push(self.parse_identifier("relationship kind")?);
        } else if matches!(self.peek(), Some(TokenKind::Identifier(_))) {
            alias = Some(self.parse_identifier("relationship alias")?);
            if self.consume_symbol(Symbol::Colon) {
                kinds.push(self.parse_identifier("relationship kind")?);
            }
        }
        while self.consume_symbol(Symbol::Pipe) {
            self.consume_symbol(Symbol::Colon);
            kinds.push(self.parse_identifier("relationship kind")?);
        }
        let mut min_hops = 1;
        let mut max_hops = 1;
        if self.consume_symbol(Symbol::Star) {
            max_hops = MAX_VARIABLE_HOPS;
            let first = if matches!(self.peek(), Some(TokenKind::Number(_))) {
                Some(self.parse_usize("relationship hop count")?)
            } else {
                None
            };
            if self.consume_symbol(Symbol::Dot) {
                self.expect_symbol(Symbol::Dot)?;
                min_hops = first.unwrap_or(1);
                if matches!(self.peek(), Some(TokenKind::Number(_))) {
                    max_hops = self.parse_usize("relationship maximum hop count")?;
                }
            } else if let Some(hops) = first {
                min_hops = hops;
                max_hops = hops;
            }
            if min_hops > MAX_VARIABLE_HOPS || max_hops > MAX_VARIABLE_HOPS {
                self.warnings.push(format!(
                    "variable-length relationship bound {min_hops}..{max_hops} was clamped to {MAX_VARIABLE_HOPS} hops"
                ));
            }
            min_hops = min_hops.min(MAX_VARIABLE_HOPS);
            max_hops = max_hops.min(MAX_VARIABLE_HOPS);
            if max_hops < min_hops {
                return Err(self.error("relationship maximum hop count is below minimum"));
            }
        }
        self.expect_symbol(Symbol::RightBracket)?;
        Ok(EdgePattern {
            alias,
            kinds,
            direction: EdgeDirection::Outbound,
            min_hops,
            max_hops,
        })
    }

    fn parse_or_expression(&mut self) -> Result<Expression, QueryError> {
        let mut expression = self.parse_xor_expression()?;
        while self.consume_keyword("OR") {
            expression =
                Expression::Or(Box::new(expression), Box::new(self.parse_xor_expression()?));
        }
        Ok(expression)
    }

    fn parse_xor_expression(&mut self) -> Result<Expression, QueryError> {
        let mut expression = self.parse_and_expression()?;
        while self.consume_keyword("XOR") {
            expression =
                Expression::Xor(Box::new(expression), Box::new(self.parse_and_expression()?));
        }
        Ok(expression)
    }

    fn parse_and_expression(&mut self) -> Result<Expression, QueryError> {
        let mut expression = self.parse_not_expression()?;
        while self.consume_keyword("AND") {
            expression =
                Expression::And(Box::new(expression), Box::new(self.parse_not_expression()?));
        }
        Ok(expression)
    }

    fn parse_not_expression(&mut self) -> Result<Expression, QueryError> {
        if self.consume_keyword("NOT") {
            return Ok(Expression::Not(Box::new(self.parse_not_expression()?)));
        }
        if self.consume_keyword("EXISTS") {
            self.expect_symbol(Symbol::LeftBrace)?;
            let patterns = self.parse_pattern_chain()?;
            self.expect_symbol(Symbol::RightBrace)?;
            return Ok(Expression::Exists(patterns));
        }
        if self.consume_symbol(Symbol::LeftParen) {
            let expression = self.parse_or_expression()?;
            self.expect_symbol(Symbol::RightParen)?;
            return Ok(expression);
        }
        Ok(Expression::Predicate(Box::new(self.parse_predicate()?)))
    }

    fn parse_predicate(&mut self) -> Result<Predicate, QueryError> {
        let left = self.parse_operand()?;
        let (operator, right) = if self.consume_symbol(Symbol::Colon) {
            let mut labels = vec![self.parse_identifier("node label")?];
            while self.consume_symbol(Symbol::Pipe) {
                self.consume_symbol(Symbol::Colon);
                labels.push(self.parse_identifier("node label")?);
            }
            (PredicateOperator::HasLabel(labels), None)
        } else if self.consume_keyword("IS") {
            let negated = self.consume_keyword("NOT");
            self.expect_keyword("NULL")?;
            (
                if negated {
                    PredicateOperator::IsNotNull
                } else {
                    PredicateOperator::IsNull
                },
                None,
            )
        } else if self.consume_keyword("CONTAINS") {
            (PredicateOperator::Contains, Some(self.parse_operand()?))
        } else if self.consume_keyword("STARTS") {
            self.expect_keyword("WITH")?;
            (PredicateOperator::StartsWith, Some(self.parse_operand()?))
        } else if self.consume_keyword("ENDS") {
            self.expect_keyword("WITH")?;
            (PredicateOperator::EndsWith, Some(self.parse_operand()?))
        } else if self.consume_keyword("IN") {
            (PredicateOperator::In, Some(self.parse_list_operand()?))
        } else if self.consume_keyword("NOT") {
            self.expect_keyword("IN")?;
            (PredicateOperator::NotIn, Some(self.parse_list_operand()?))
        } else {
            let operator = if self.consume_symbol(Symbol::Equal) {
                PredicateOperator::Equal
            } else if self.consume_symbol(Symbol::NotEqual) {
                PredicateOperator::NotEqual
            } else if self.consume_symbol(Symbol::LessEqual) {
                PredicateOperator::LessEqual
            } else if self.consume_symbol(Symbol::GreaterEqual) {
                PredicateOperator::GreaterEqual
            } else if self.consume_symbol(Symbol::Less) {
                PredicateOperator::Less
            } else if self.consume_symbol(Symbol::Greater) {
                PredicateOperator::Greater
            } else if self.consume_symbol(Symbol::Regex) {
                PredicateOperator::Regex
            } else {
                return Err(self.error("expected predicate operator"));
            };
            (operator, Some(self.parse_operand()?))
        };
        Ok(Predicate {
            left,
            operator,
            right,
        })
    }

    fn parse_operand(&mut self) -> Result<Operand, QueryError> {
        if matches!(self.peek(), Some(TokenKind::Symbol(Symbol::LeftBracket))) {
            return self.parse_list_operand();
        }
        if self.peek_function_call() {
            return self.parse_function_operand();
        }
        if self.consume_keyword("TRUE") {
            return Ok(Operand::Literal(Box::new(QueryValue::Bool(true))));
        }
        if self.consume_keyword("FALSE") {
            return Ok(Operand::Literal(Box::new(QueryValue::Bool(false))));
        }
        if self.consume_keyword("NULL") {
            return Ok(Operand::Literal(Box::new(QueryValue::Null)));
        }
        if let Some(TokenKind::String(value)) = self.peek().cloned() {
            self.index += 1;
            return Ok(Operand::Literal(Box::new(QueryValue::String(value))));
        }
        let negative = self.consume_symbol(Symbol::Dash);
        if let Some(TokenKind::Number(value)) = self.peek().cloned() {
            self.index += 1;
            return parse_number(&value, negative).map(|value| Operand::Literal(Box::new(value)));
        }
        if negative {
            return Err(self.error("expected number after unary minus"));
        }
        Ok(Operand::Reference(self.parse_reference()?))
    }

    fn parse_function_operand(&mut self) -> Result<Operand, QueryError> {
        let name = self.parse_identifier("function name")?;
        self.expect_symbol(Symbol::LeftParen)?;
        let mut arguments = Vec::new();
        if !self.consume_symbol(Symbol::RightParen) {
            loop {
                arguments.push(self.parse_operand()?);
                if !self.consume_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RightParen)?;
        }
        Ok(Operand::Function { name, arguments })
    }

    fn parse_list_operand(&mut self) -> Result<Operand, QueryError> {
        self.expect_symbol(Symbol::LeftBracket)?;
        let mut values = Vec::new();
        if !self.consume_symbol(Symbol::RightBracket) {
            loop {
                values.push(self.parse_operand()?);
                if !self.consume_symbol(Symbol::Comma) {
                    break;
                }
            }
            self.expect_symbol(Symbol::RightBracket)?;
        }
        Ok(Operand::List(values))
    }

    fn parse_projections(&mut self) -> Result<Vec<Projection>, QueryError> {
        let mut projections = Vec::new();
        loop {
            projections.push(self.parse_projection()?);
            if projections.len() > MAX_PROJECTIONS {
                return Err(unsupported("query exceeds projection safety cap"));
            }
            if !self.consume_symbol(Symbol::Comma) {
                break;
            }
        }
        if projections.is_empty() {
            return Err(self.error("RETURN requires at least one expression"));
        }
        Ok(projections)
    }

    fn parse_projection(&mut self) -> Result<Projection, QueryError> {
        let aggregate = if self.consume_keyword("COUNT") {
            Some((AggregateKind::Count, "count"))
        } else if self.consume_keyword("SUM") {
            Some((AggregateKind::Sum, "sum"))
        } else if self.consume_keyword("AVG") {
            Some((AggregateKind::Average, "avg"))
        } else if self.consume_keyword("MIN") {
            Some((AggregateKind::Minimum, "min"))
        } else if self.consume_keyword("MAX") {
            Some((AggregateKind::Maximum, "max"))
        } else if self.consume_keyword("COLLECT") {
            Some((AggregateKind::Collect, "collect"))
        } else {
            None
        };
        let (expression, default_column) = if let Some((kind, name)) = aggregate {
            self.expect_symbol(Symbol::LeftParen)?;
            let distinct = self.consume_keyword("DISTINCT");
            let target = if self.consume_symbol(Symbol::Star) {
                None
            } else {
                Some(self.parse_reference()?)
            };
            self.expect_symbol(Symbol::RightParen)?;
            if target.is_none() && !matches!(kind, AggregateKind::Count) {
                return Err(self.error("only COUNT accepts *"));
            }
            let target_column = target
                .as_ref()
                .map_or_else(|| "*".to_owned(), reference_column);
            let distinct_prefix = if distinct { "DISTINCT " } else { "" };
            let column = format!("{name}({distinct_prefix}{target_column})");
            (
                ProjectionExpression::Aggregate {
                    kind,
                    target,
                    distinct,
                },
                column,
            )
        } else if self.consume_keyword("CASE") {
            let expression = self.parse_case_expression()?;
            (ProjectionExpression::Case(expression), "case".to_owned())
        } else if self.peek_function_call() {
            let Operand::Function { name, arguments } = self.parse_function_operand()? else {
                unreachable!();
            };
            let column = function_column(&name, &arguments);
            (ProjectionExpression::Function { name, arguments }, column)
        } else {
            let reference = self.parse_reference()?;
            let column = reference_column(&reference);
            (ProjectionExpression::Reference(reference), column)
        };
        let column = if self.consume_keyword("AS") {
            self.parse_identifier("projection alias")?
        } else {
            default_column
        };
        Ok(Projection { expression, column })
    }

    fn parse_case_expression(&mut self) -> Result<CaseExpression, QueryError> {
        let subject = if self.consume_keyword("WHEN") {
            None
        } else {
            let subject = self.parse_operand()?;
            self.expect_keyword("WHEN")?;
            Some(subject)
        };
        let mut branches = Vec::new();
        loop {
            let when = if subject.is_some() {
                CaseWhen::Value(self.parse_operand()?)
            } else {
                CaseWhen::Predicate(self.parse_or_expression()?)
            };
            self.expect_keyword("THEN")?;
            let then = self.parse_operand()?;
            branches.push(CaseBranch { when, then });
            if !self.consume_keyword("WHEN") {
                break;
            }
        }
        let fallback = if self.consume_keyword("ELSE") {
            Some(self.parse_operand()?)
        } else {
            None
        };
        self.expect_keyword("END")?;
        Ok(CaseExpression {
            subject,
            branches,
            fallback,
        })
    }

    fn parse_order_clauses(&mut self) -> Result<Vec<OrderClause>, QueryError> {
        let mut clauses = Vec::new();
        loop {
            let reference = self.parse_reference()?;
            let descending = if self.consume_keyword("DESC") {
                true
            } else {
                self.consume_keyword("ASC");
                false
            };
            clauses.push(OrderClause {
                reference,
                descending,
            });
            if !self.consume_symbol(Symbol::Comma) {
                break;
            }
        }
        Ok(clauses)
    }

    fn parse_reference(&mut self) -> Result<Reference, QueryError> {
        if self.consume_keyword("TYPE") {
            self.expect_symbol(Symbol::LeftParen)?;
            let alias = self.parse_identifier("relationship alias")?;
            self.expect_symbol(Symbol::RightParen)?;
            return Ok(Reference::EdgeType(alias));
        }
        let alias = self.parse_identifier("alias")?;
        let mut path = Vec::new();
        while self.consume_symbol(Symbol::Dot) {
            path.push(self.parse_identifier("property")?);
        }
        if path.is_empty() {
            Ok(Reference::Alias(alias))
        } else {
            Ok(Reference::Property { alias, path })
        }
    }

    fn parse_identifier(&mut self, expected: &str) -> Result<String, QueryError> {
        match self.peek().cloned() {
            Some(TokenKind::Identifier(identifier)) => {
                self.index += 1;
                Ok(identifier)
            }
            _ => Err(self.error(&format!("expected {expected}"))),
        }
    }

    fn parse_usize(&mut self, clause: &str) -> Result<usize, QueryError> {
        match self.peek().cloned() {
            Some(TokenKind::Number(value)) if !value.contains('.') => {
                self.index += 1;
                value
                    .parse()
                    .map_err(|_| self.error(&format!("invalid {clause} value")))
            }
            _ => Err(self.error(&format!("{clause} requires a non-negative integer"))),
        }
    }

    fn expect_keyword(&mut self, keyword: &str) -> Result<(), QueryError> {
        if self.consume_keyword(keyword) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {keyword}")))
        }
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        let matches = matches!(
            self.peek(),
            Some(TokenKind::Identifier(identifier)) if identifier.eq_ignore_ascii_case(keyword)
        );
        if matches {
            self.index += 1;
        }
        matches
    }

    fn expect_symbol(&mut self, symbol: Symbol) -> Result<(), QueryError> {
        if self.consume_symbol(symbol) {
            Ok(())
        } else {
            Err(self.error("unexpected token"))
        }
    }

    fn consume_symbol(&mut self, symbol: Symbol) -> bool {
        let matches = matches!(self.peek(), Some(TokenKind::Symbol(actual)) if *actual == symbol);
        if matches {
            self.index += 1;
        }
        matches
    }

    fn peek(&self) -> Option<&TokenKind> {
        self.tokens.get(self.index).map(|token| &token.kind)
    }

    fn peek_function_call(&self) -> bool {
        matches!(self.peek(), Some(TokenKind::Identifier(_)))
            && matches!(
                self.tokens.get(self.index + 1).map(|token| &token.kind),
                Some(TokenKind::Symbol(Symbol::LeftParen))
            )
    }

    fn error(&self, message: &str) -> QueryError {
        let position = self
            .tokens
            .get(self.index)
            .map_or(self.end_position, |token| token.position);
        syntax(position, message)
    }
}

fn parse_number(value: &str, negative: bool) -> Result<QueryValue, QueryError> {
    let sign = if negative { "-" } else { "" };
    let number = format!("{sign}{value}");
    if value.contains('.') {
        return number
            .parse::<f64>()
            .map(QueryValue::Float)
            .map_err(|_| syntax(0, "invalid floating-point literal"));
    }
    number
        .parse::<i64>()
        .map(QueryValue::Integer)
        .map_err(|_| syntax(0, "invalid integer literal"))
}

fn expand_star_projections(matches: &[MatchClause]) -> Vec<Projection> {
    let mut seen = BTreeSet::new();
    let aliases: Vec<&str> = matches
        .iter()
        .flat_map(|clause| clause.patterns.iter().flat_map(pattern_node_aliases))
        .filter(|alias| !alias.starts_with("__anon_node_"))
        .filter(|alias| seen.insert((*alias).to_owned()))
        .collect();
    aliases
        .into_iter()
        .flat_map(|alias| {
            ["name", "qualified_name", "label", "file_path"].map(|property| {
                let reference = Reference::Property {
                    alias: alias.to_owned(),
                    path: vec![property.to_owned()],
                };
                Projection {
                    column: reference_column(&reference),
                    expression: ProjectionExpression::Reference(reference),
                }
            })
        })
        .collect()
}

fn expand_with_star_projections(clause: &WithClause) -> Vec<Projection> {
    clause
        .projections
        .iter()
        .map(|projection| Projection {
            expression: ProjectionExpression::Reference(Reference::Alias(
                projection.column.clone(),
            )),
            column: projection.column.clone(),
        })
        .collect()
}

fn pattern_node_aliases(pattern: &MatchPattern) -> Vec<&str> {
    match pattern {
        MatchPattern::Node(node) => vec![node.alias.as_str()],
        MatchPattern::Edge(edge) => vec![edge.left.alias.as_str(), edge.right.alias.as_str()],
    }
}

fn reference_column(reference: &Reference) -> String {
    match reference {
        Reference::Alias(alias) => alias.clone(),
        Reference::Property { alias, path } => format!("{alias}.{}", path.join(".")),
        Reference::EdgeType(alias) => format!("type({alias})"),
    }
}

fn function_column(name: &str, arguments: &[Operand]) -> String {
    format!(
        "{name}({})",
        arguments
            .iter()
            .map(operand_column)
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn operand_column(operand: &Operand) -> String {
    match operand {
        Operand::Literal(value) => match value.as_ref() {
            QueryValue::Null => "null".to_owned(),
            QueryValue::Bool(value) => value.to_string(),
            QueryValue::Integer(value) => value.to_string(),
            QueryValue::Unsigned(value) => value.to_string(),
            QueryValue::Float(value) => value.to_string(),
            QueryValue::String(value) => format!("'{value}'"),
            QueryValue::Json(value) => value.to_string(),
            QueryValue::Node(_) | QueryValue::Edge(_) => "entity".to_owned(),
        },
        Operand::List(values) => format!(
            "[{}]",
            values
                .iter()
                .map(operand_column)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Operand::Reference(reference) => reference_column(reference),
        Operand::Function { name, arguments } => function_column(name, arguments),
    }
}

fn syntax(position: usize, message: &str) -> QueryError {
    QueryError::CypherSyntax {
        position,
        message: message.to_owned(),
    }
}

fn unsupported(message: &str) -> QueryError {
    QueryError::UnsupportedQuery {
        message: message.to_owned(),
    }
}

#[derive(Clone, Default)]
struct Binding<'a> {
    nodes: BTreeMap<String, &'a GraphNode>,
    edges: BTreeMap<String, &'a GraphEdge>,
    values: BTreeMap<String, QueryValue>,
    all_nodes: Option<&'a [GraphNode]>,
    all_edges: Option<&'a [GraphEdge]>,
}

fn execute_unwind<'a>(
    clause: Option<&UnwindClause>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    let Some(clause) = clause else {
        return Ok(vec![Binding::default()]);
    };
    let seed = Binding::default();
    let value = evaluate_operand(&clause.expression, &seed, degrees)?;
    let values = match value {
        QueryValue::Json(serde_json::Value::Array(values)) => {
            values.iter().map(json_value).collect::<Vec<_>>()
        }
        QueryValue::Null => Vec::new(),
        _ => return Err(unsupported("UNWIND expression must evaluate to a list")),
    };
    if values.len() > MAX_INTERMEDIATE_BINDINGS {
        return Err(unsupported(
            "UNWIND exceeds intermediate binding safety cap",
        ));
    }
    Ok(values
        .into_iter()
        .map(|value| Binding {
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
            values: BTreeMap::from([(clause.alias.clone(), value)]),
            all_nodes: None,
            all_edges: None,
        })
        .collect())
}

fn execute_match_clauses<'a>(
    clauses: &[MatchClause],
    mut bindings: Vec<Binding<'a>>,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    for clause in clauses {
        let candidate_sets = clause
            .patterns
            .iter()
            .map(|pattern| build_bindings_bounded(pattern, nodes, edges, degrees))
            .collect::<Result<Vec<_>, _>>()?;
        let mut joined = Vec::new();
        for binding in &bindings {
            let mut partial = vec![binding.clone()];
            for candidates in &candidate_sets {
                let mut next = Vec::new();
                for binding in &partial {
                    for candidate in candidates {
                        if let Some(merged) = merge_bindings(binding, candidate) {
                            next.push(merged);
                            if next.len() > MAX_INTERMEDIATE_BINDINGS {
                                return Err(unsupported(
                                    "query exceeds intermediate binding safety cap",
                                ));
                            }
                        }
                    }
                }
                partial = next;
                if partial.is_empty() {
                    break;
                }
            }
            if clause.optional && partial.is_empty() {
                let mut unmatched = binding.clone();
                for pattern in &clause.patterns {
                    mark_pattern_aliases_null(&mut unmatched, pattern);
                }
                joined.push(unmatched);
            } else {
                joined.extend(partial);
            }
        }
        if let Some(filter) = &clause.filter {
            let mut retained = Vec::with_capacity(joined.len());
            for binding in joined {
                if evaluate_expression(filter, &binding, degrees)? {
                    retained.push(binding);
                }
            }
            joined = retained;
        }
        bindings = joined;
    }
    Ok(bindings)
}

fn merge_bindings<'a>(base: &Binding<'a>, candidate: &Binding<'a>) -> Option<Binding<'a>> {
    let mut merged = base.clone();
    if merged.all_nodes.is_none() {
        merged.all_nodes = candidate.all_nodes;
    }
    if merged.all_edges.is_none() {
        merged.all_edges = candidate.all_edges;
    }
    for (alias, node) in &candidate.nodes {
        if merged
            .nodes
            .get(alias)
            .is_some_and(|existing| existing.id != node.id)
            || merged.edges.contains_key(alias)
            || merged
                .values
                .get(alias)
                .is_some_and(|value| !matches!(value, QueryValue::Null))
        {
            return None;
        }
        merged.values.remove(alias);
        merged.nodes.insert(alias.clone(), node);
    }
    for (alias, edge) in &candidate.edges {
        if merged.edges.get(alias).is_some_and(|existing| {
            existing.source != edge.source
                || existing.target != edge.target
                || existing.kind != edge.kind
                || existing.discriminator != edge.discriminator
        }) || merged.nodes.contains_key(alias)
            || merged
                .values
                .get(alias)
                .is_some_and(|value| !matches!(value, QueryValue::Null))
        {
            return None;
        }
        merged.values.remove(alias);
        merged.edges.insert(alias.clone(), edge);
    }
    for (alias, value) in &candidate.values {
        if let Some(existing) = merged.values.get(alias)
            && !values_equal(existing, value)
        {
            return None;
        }
        merged.values.insert(alias.clone(), value.clone());
    }
    Some(merged)
}

fn mark_pattern_aliases_null(binding: &mut Binding<'_>, pattern: &MatchPattern) {
    let mut aliases: Vec<&str> = pattern_node_aliases(pattern);
    if let MatchPattern::Edge(edge) = pattern
        && let Some(alias) = edge.edge.alias.as_deref()
    {
        aliases.push(alias);
    }
    for alias in aliases {
        if !binding.nodes.contains_key(alias) && !binding.edges.contains_key(alias) {
            binding.values.insert(alias.to_owned(), QueryValue::Null);
        }
    }
}

fn build_bindings_bounded<'a>(
    pattern: &MatchPattern,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    let mut bindings = match pattern {
        MatchPattern::Node(pattern) => nodes
            .iter()
            .filter(|node| node_matches(node, pattern, degrees))
            .map(|node| Binding {
                nodes: BTreeMap::from([(pattern.alias.clone(), node)]),
                edges: BTreeMap::new(),
                values: BTreeMap::new(),
                all_nodes: Some(nodes),
                all_edges: Some(edges),
            })
            .collect(),
        MatchPattern::Edge(pattern) => {
            let EdgeMatch { left, edge, right } = pattern.as_ref();
            if edge.min_hops != 1 || edge.max_hops != 1 {
                return build_variable_bindings(pattern, nodes, edges, degrees);
            }
            let nodes_by_id: BTreeMap<&NodeId, &GraphNode> =
                nodes.iter().map(|node| (&node.id, node)).collect();
            let mut bindings = Vec::new();
            for graph_edge in edges.iter().filter(|graph_edge| {
                edge.kinds.is_empty()
                    || edge
                        .kinds
                        .iter()
                        .any(|kind| graph_edge.kind.as_str() == kind)
            }) {
                let Some(source) = nodes_by_id.get(&graph_edge.source).copied() else {
                    continue;
                };
                let Some(target) = nodes_by_id.get(&graph_edge.target).copied() else {
                    continue;
                };
                match edge.direction {
                    EdgeDirection::Outbound => push_edge_binding(
                        &mut bindings,
                        left,
                        right,
                        edge,
                        source,
                        target,
                        graph_edge,
                        degrees,
                    ),
                    EdgeDirection::Inbound => push_edge_binding(
                        &mut bindings,
                        left,
                        right,
                        edge,
                        target,
                        source,
                        graph_edge,
                        degrees,
                    ),
                    EdgeDirection::Undirected => {
                        push_edge_binding(
                            &mut bindings,
                            left,
                            right,
                            edge,
                            source,
                            target,
                            graph_edge,
                            degrees,
                        );
                        if source.id != target.id {
                            push_edge_binding(
                                &mut bindings,
                                left,
                                right,
                                edge,
                                target,
                                source,
                                graph_edge,
                                degrees,
                            );
                        }
                    }
                }
            }
            bindings
        }
    };
    if bindings.len() > MAX_INTERMEDIATE_BINDINGS {
        return Err(unsupported("query exceeds intermediate binding safety cap"));
    }
    for binding in &mut bindings {
        binding.all_nodes = Some(nodes);
        binding.all_edges = Some(edges);
    }
    Ok(bindings)
}

fn push_edge_binding<'a>(
    bindings: &mut Vec<Binding<'a>>,
    left_pattern: &NodePattern,
    right_pattern: &NodePattern,
    edge_pattern: &EdgePattern,
    left: &'a GraphNode,
    right: &'a GraphNode,
    edge: &'a GraphEdge,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) {
    if !node_matches(left, left_pattern, degrees) || !node_matches(right, right_pattern, degrees) {
        return;
    }
    if left_pattern.alias == right_pattern.alias && left.id != right.id {
        return;
    }
    let mut edges = BTreeMap::new();
    if let Some(alias) = &edge_pattern.alias {
        edges.insert(alias.clone(), edge);
    }
    bindings.push(Binding {
        nodes: BTreeMap::from([
            (left_pattern.alias.clone(), left),
            (right_pattern.alias.clone(), right),
        ]),
        edges,
        values: BTreeMap::new(),
        all_nodes: None,
        all_edges: None,
    });
}

fn build_variable_bindings<'a>(
    pattern: &EdgeMatch,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    struct Frame<'a> {
        start: &'a GraphNode,
        current: &'a GraphNode,
        depth: usize,
        used_edges: BTreeSet<usize>,
        last_edge: Option<&'a GraphEdge>,
    }

    let nodes_by_id: BTreeMap<&NodeId, &GraphNode> =
        nodes.iter().map(|node| (&node.id, node)).collect();
    let mut bindings = Vec::new();
    for start in nodes
        .iter()
        .filter(|node| node_matches(node, &pattern.left, degrees))
    {
        let mut stack = vec![Frame {
            start,
            current: start,
            depth: 0,
            used_edges: BTreeSet::new(),
            last_edge: None,
        }];
        while let Some(frame) = stack.pop() {
            if frame.depth >= pattern.edge.min_hops
                && node_matches(frame.current, &pattern.right, degrees)
                && (pattern.left.alias != pattern.right.alias || frame.start.id == frame.current.id)
            {
                let mut bound_edges = BTreeMap::new();
                if let (Some(alias), Some(edge)) = (&pattern.edge.alias, frame.last_edge) {
                    bound_edges.insert(alias.clone(), edge);
                }
                bindings.push(Binding {
                    nodes: BTreeMap::from([
                        (pattern.left.alias.clone(), frame.start),
                        (pattern.right.alias.clone(), frame.current),
                    ]),
                    edges: bound_edges,
                    values: BTreeMap::new(),
                    all_nodes: Some(nodes),
                    all_edges: Some(edges),
                });
                if bindings.len() > MAX_INTERMEDIATE_BINDINGS {
                    return Err(unsupported("query exceeds intermediate binding safety cap"));
                }
            }
            if frame.depth >= pattern.edge.max_hops {
                continue;
            }
            for (edge_index, edge) in edges.iter().enumerate().rev() {
                if frame.used_edges.contains(&edge_index)
                    || (!pattern.edge.kinds.is_empty()
                        && !pattern
                            .edge
                            .kinds
                            .iter()
                            .any(|kind| edge.kind.as_str() == kind))
                {
                    continue;
                }
                let next_ids: Vec<&NodeId> = match pattern.edge.direction {
                    EdgeDirection::Outbound | EdgeDirection::Undirected
                        if edge.source == frame.current.id =>
                    {
                        vec![&edge.target]
                    }
                    EdgeDirection::Inbound | EdgeDirection::Undirected
                        if edge.target == frame.current.id =>
                    {
                        vec![&edge.source]
                    }
                    _ => Vec::new(),
                };
                for next_id in next_ids {
                    let Some(next) = nodes_by_id.get(next_id).copied() else {
                        continue;
                    };
                    let mut used_edges = frame.used_edges.clone();
                    used_edges.insert(edge_index);
                    stack.push(Frame {
                        start: frame.start,
                        current: next,
                        depth: frame.depth + 1,
                        used_edges,
                        last_edge: Some(edge),
                    });
                }
            }
        }
    }
    Ok(bindings)
}

fn node_matches(
    node: &GraphNode,
    pattern: &NodePattern,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> bool {
    (pattern.labels.is_empty()
        || pattern
            .labels
            .iter()
            .any(|label| node.label.as_str() == label))
        && pattern.properties.iter().all(|(property, expected)| {
            values_equal(
                &node_property(node, std::slice::from_ref(property), degrees),
                expected,
            )
        })
}

fn evaluate_expression(
    expression: &Expression,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<bool, QueryError> {
    match expression {
        Expression::And(left, right) => Ok(evaluate_expression(left, binding, degrees)?
            && evaluate_expression(right, binding, degrees)?),
        Expression::Or(left, right) => Ok(evaluate_expression(left, binding, degrees)?
            || evaluate_expression(right, binding, degrees)?),
        Expression::Xor(left, right) => Ok(evaluate_expression(left, binding, degrees)?
            ^ evaluate_expression(right, binding, degrees)?),
        Expression::Not(inner) => Ok(!evaluate_expression(inner, binding, degrees)?),
        Expression::Exists(patterns) => evaluate_exists(patterns, binding, degrees),
        Expression::Predicate(predicate) => evaluate_predicate(predicate, binding, degrees),
    }
}

fn evaluate_exists(
    patterns: &[MatchPattern],
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<bool, QueryError> {
    let (Some(nodes), Some(edges)) = (binding.all_nodes, binding.all_edges) else {
        return Ok(false);
    };
    let mut partial = vec![binding.clone()];
    for pattern in patterns {
        let candidates = build_bindings_bounded(pattern, nodes, edges, degrees)?;
        let mut next = Vec::new();
        for binding in &partial {
            for candidate in &candidates {
                if let Some(merged) = merge_bindings(binding, candidate) {
                    next.push(merged);
                    if next.len() > MAX_INTERMEDIATE_BINDINGS {
                        return Err(unsupported(
                            "EXISTS exceeds intermediate binding safety cap",
                        ));
                    }
                }
            }
        }
        partial = next;
        if partial.is_empty() {
            return Ok(false);
        }
    }
    Ok(!partial.is_empty())
}

fn evaluate_predicate(
    predicate: &Predicate,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<bool, QueryError> {
    let left = evaluate_operand(&predicate.left, binding, degrees)?;
    if let PredicateOperator::HasLabel(labels) = &predicate.operator {
        return Ok(matches!(
            left,
            QueryValue::Node(ref node) if labels.contains(&node.label)
        ));
    }
    if matches!(&predicate.operator, PredicateOperator::IsNull) {
        return Ok(matches!(left, QueryValue::Null));
    }
    if matches!(&predicate.operator, PredicateOperator::IsNotNull) {
        return Ok(!matches!(left, QueryValue::Null));
    }
    let right = evaluate_operand(
        predicate
            .right
            .as_ref()
            .expect("binary predicate has right operand"),
        binding,
        degrees,
    )?;
    if matches!(left, QueryValue::Null) || matches!(right, QueryValue::Null) {
        return Ok(false);
    }
    if matches!(&predicate.operator, PredicateOperator::Regex) {
        let (QueryValue::String(value), QueryValue::String(pattern)) = (&left, &right) else {
            return Ok(false);
        };
        let regex = Regex::new(pattern)
            .map_err(|error| unsupported(&format!("invalid regular expression: {error}")))?;
        return Ok(regex.is_match(value));
    }
    Ok(match &predicate.operator {
        PredicateOperator::Equal => values_equal(&left, &right),
        PredicateOperator::NotEqual => !values_equal(&left, &right),
        PredicateOperator::Less => compare_values(&left, &right) == Ordering::Less,
        PredicateOperator::LessEqual => compare_values(&left, &right) != Ordering::Greater,
        PredicateOperator::Greater => compare_values(&left, &right) == Ordering::Greater,
        PredicateOperator::GreaterEqual => compare_values(&left, &right) != Ordering::Less,
        PredicateOperator::In | PredicateOperator::NotIn => {
            let Operand::List(items) = predicate
                .right
                .as_ref()
                .expect("IN predicate has list operand")
            else {
                unreachable!();
            };
            let contains = items.iter().try_fold(false, |matched, item| {
                Ok::<_, QueryError>(
                    matched || values_equal(&left, &evaluate_operand(item, binding, degrees)?),
                )
            })?;
            if matches!(&predicate.operator, PredicateOperator::NotIn) {
                !contains
            } else {
                contains
            }
        }
        PredicateOperator::Contains => {
            string_pair(&left, &right, |left, right| left.contains(right))
        }
        PredicateOperator::StartsWith => {
            string_pair(&left, &right, |left, right| left.starts_with(right))
        }
        PredicateOperator::EndsWith => {
            string_pair(&left, &right, |left, right| left.ends_with(right))
        }
        PredicateOperator::Regex
        | PredicateOperator::HasLabel(_)
        | PredicateOperator::IsNull
        | PredicateOperator::IsNotNull => unreachable!(),
    })
}

fn evaluate_operand(
    operand: &Operand,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    match operand {
        Operand::Literal(value) => Ok(value.as_ref().clone()),
        Operand::List(values) => Ok(QueryValue::Json(serde_json::Value::Array(
            values
                .iter()
                .map(|value| {
                    evaluate_operand(value, binding, degrees).and_then(query_value_to_json)
                })
                .collect::<Result<_, _>>()?,
        ))),
        Operand::Reference(reference) => evaluate_reference(reference, binding, degrees),
        Operand::Function { name, arguments } => {
            evaluate_scalar_function(name, arguments, binding, degrees)
        }
    }
}

fn evaluate_scalar_function(
    name: &str,
    arguments: &[Operand],
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    let values = arguments
        .iter()
        .map(|argument| evaluate_operand(argument, binding, degrees))
        .collect::<Result<Vec<_>, _>>()?;
    let normalized = name.to_ascii_lowercase();
    let value = match normalized.as_str() {
        "coalesce" => values
            .into_iter()
            .find(|value| !matches!(value, QueryValue::Null))
            .unwrap_or(QueryValue::Null),
        "tolower" => unary_string(&values, str::to_lowercase),
        "toupper" => unary_string(&values, str::to_uppercase),
        "tostring" => values
            .first()
            .and_then(query_value_string)
            .map_or(QueryValue::Null, QueryValue::String),
        "tointeger" => values.first().map_or(QueryValue::Null, query_value_integer),
        "tofloat" => values.first().map_or(QueryValue::Null, query_value_float),
        "toboolean" => values.first().map_or(QueryValue::Null, query_value_boolean),
        "size" | "length" => values.first().map_or(QueryValue::Null, value_size),
        "reverse" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::String(value) => QueryValue::String(value.chars().rev().collect()),
                QueryValue::Json(serde_json::Value::Array(values)) => {
                    let mut values = values.clone();
                    values.reverse();
                    QueryValue::Json(serde_json::Value::Array(values))
                }
                _ => QueryValue::Null,
            }),
        "trim" => unary_string(&values, |value| value.trim().to_owned()),
        "ltrim" => unary_string(&values, |value| value.trim_start().to_owned()),
        "rtrim" => unary_string(&values, |value| value.trim_end().to_owned()),
        "substring" => substring_value(&values),
        "left" => edge_slice_value(&values, true),
        "right" => edge_slice_value(&values, false),
        "replace" => match (values.first(), values.get(1), values.get(2)) {
            (
                Some(QueryValue::String(value)),
                Some(QueryValue::String(from)),
                Some(QueryValue::String(to)),
            ) => QueryValue::String(value.replace(from, to)),
            _ => QueryValue::Null,
        },
        "split" => match (values.first(), values.get(1)) {
            (Some(QueryValue::String(value)), Some(QueryValue::String(separator))) => {
                let parts: Vec<String> = if separator.is_empty() {
                    value
                        .chars()
                        .map(|character| character.to_string())
                        .collect()
                } else {
                    value.split(separator).map(str::to_owned).collect()
                };
                QueryValue::Json(serde_json::Value::Array(
                    parts.into_iter().map(serde_json::Value::String).collect(),
                ))
            }
            _ => QueryValue::Null,
        },
        "labels" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Node(node) => {
                    QueryValue::Json(serde_json::Value::Array(vec![serde_json::Value::String(
                        node.label.clone(),
                    )]))
                }
                _ => QueryValue::Json(serde_json::Value::Array(Vec::new())),
            }),
        "type" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Edge(edge) => QueryValue::String(edge.kind.clone()),
                _ => QueryValue::Null,
            }),
        "id" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Node(node) => QueryValue::String(node.id.clone()),
                QueryValue::Edge(edge) => QueryValue::String(edge.discriminator.clone()),
                _ => QueryValue::Null,
            }),
        "properties" => values
            .first()
            .map_or(QueryValue::Null, |value| match value {
                QueryValue::Node(node) => QueryValue::Json(
                    serde_json::to_value(&node.properties).unwrap_or(serde_json::Value::Null),
                ),
                QueryValue::Edge(edge) => QueryValue::Json(
                    serde_json::to_value(&edge.properties).unwrap_or(serde_json::Value::Null),
                ),
                _ => QueryValue::Json(serde_json::Value::Object(serde_json::Map::new())),
            }),
        "keys" => values.first().map_or(QueryValue::Null, entity_keys),
        _ => return Err(unsupported(&format!("unsupported function {name}"))),
    };
    Ok(value)
}

fn unary_string(values: &[QueryValue], transform: impl FnOnce(&str) -> String) -> QueryValue {
    match values.first() {
        Some(QueryValue::String(value)) => QueryValue::String(transform(value)),
        _ => QueryValue::Null,
    }
}

fn query_value_string(value: &QueryValue) -> Option<String> {
    Some(match value {
        QueryValue::Null => return None,
        QueryValue::Bool(value) => value.to_string(),
        QueryValue::Integer(value) => value.to_string(),
        QueryValue::Unsigned(value) => value.to_string(),
        QueryValue::Float(value) => value.to_string(),
        QueryValue::String(value) => value.clone(),
        QueryValue::Json(value) => value.to_string(),
        QueryValue::Node(value) => serde_json::to_string(value).ok()?,
        QueryValue::Edge(value) => serde_json::to_string(value).ok()?,
    })
}

fn query_value_integer(value: &QueryValue) -> QueryValue {
    match value {
        QueryValue::Integer(value) => QueryValue::Integer(*value),
        QueryValue::Unsigned(value) => {
            i64::try_from(*value).map_or(QueryValue::Null, QueryValue::Integer)
        }
        QueryValue::Float(value)
            if value.is_finite() && *value >= i64::MIN as f64 && *value <= i64::MAX as f64 =>
        {
            QueryValue::Integer(value.trunc() as i64)
        }
        QueryValue::String(value) => value
            .parse::<i64>()
            .map_or(QueryValue::Null, QueryValue::Integer),
        QueryValue::Bool(value) => QueryValue::Integer(i64::from(*value)),
        _ => QueryValue::Null,
    }
}

fn query_value_float(value: &QueryValue) -> QueryValue {
    match value {
        QueryValue::Integer(value) => QueryValue::Float(*value as f64),
        QueryValue::Unsigned(value) => QueryValue::Float(*value as f64),
        QueryValue::Float(value) => QueryValue::Float(*value),
        QueryValue::String(value) => value
            .parse::<f64>()
            .map_or(QueryValue::Null, QueryValue::Float),
        QueryValue::Bool(value) => QueryValue::Float(if *value { 1.0 } else { 0.0 }),
        _ => QueryValue::Null,
    }
}

fn query_value_boolean(value: &QueryValue) -> QueryValue {
    match value {
        QueryValue::Bool(value) => QueryValue::Bool(*value),
        QueryValue::Integer(value) => QueryValue::Bool(*value != 0),
        QueryValue::Unsigned(value) => QueryValue::Bool(*value != 0),
        QueryValue::Float(value) => QueryValue::Bool(*value != 0.0),
        QueryValue::String(value) if value.eq_ignore_ascii_case("true") => QueryValue::Bool(true),
        QueryValue::String(value) if value.eq_ignore_ascii_case("false") => QueryValue::Bool(false),
        _ => QueryValue::Null,
    }
}

fn value_size(value: &QueryValue) -> QueryValue {
    let size = match value {
        QueryValue::String(value) => value.chars().count(),
        QueryValue::Json(serde_json::Value::Array(values)) => values.len(),
        QueryValue::Json(serde_json::Value::Object(values)) => values.len(),
        _ => return QueryValue::Null,
    };
    i64::try_from(size).map_or(QueryValue::Null, QueryValue::Integer)
}

fn substring_value(values: &[QueryValue]) -> QueryValue {
    let (Some(QueryValue::String(value)), Some(start)) =
        (values.first(), values.get(1).and_then(value_index))
    else {
        return QueryValue::Null;
    };
    let characters: Vec<char> = value.chars().collect();
    if start >= characters.len() {
        return QueryValue::String(String::new());
    }
    let length = values
        .get(2)
        .and_then(value_index)
        .unwrap_or(characters.len() - start);
    QueryValue::String(
        characters[start..characters.len().min(start.saturating_add(length))]
            .iter()
            .collect(),
    )
}

fn edge_slice_value(values: &[QueryValue], from_left: bool) -> QueryValue {
    let (Some(QueryValue::String(value)), Some(length)) =
        (values.first(), values.get(1).and_then(value_index))
    else {
        return QueryValue::Null;
    };
    let characters: Vec<char> = value.chars().collect();
    let take = length.min(characters.len());
    let start = if from_left {
        0
    } else {
        characters.len() - take
    };
    QueryValue::String(characters[start..start + take].iter().collect())
}

fn value_index(value: &QueryValue) -> Option<usize> {
    match value {
        QueryValue::Integer(value) => usize::try_from(*value).ok(),
        QueryValue::Unsigned(value) => usize::try_from(*value).ok(),
        QueryValue::Float(value) if value.is_finite() && *value >= 0.0 => {
            usize::try_from(value.trunc() as u64).ok()
        }
        QueryValue::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn entity_keys(value: &QueryValue) -> QueryValue {
    let keys = match value {
        QueryValue::Node(node) => {
            let mut keys = vec!["name", "qualified_name", "label"];
            if node.file_path.is_some() {
                keys.push("file_path");
            }
            if node.start_line.is_some() {
                keys.push("start_line");
            }
            if node.end_line.is_some() {
                keys.push("end_line");
            }
            let mut keys: Vec<String> = keys.into_iter().map(str::to_owned).collect();
            keys.extend(node.properties.keys().cloned());
            keys
        }
        QueryValue::Edge(edge) => {
            let mut keys = vec![
                "source_id".to_owned(),
                "target_id".to_owned(),
                "kind".to_owned(),
                "discriminator".to_owned(),
            ];
            keys.extend(edge.properties.keys().cloned());
            keys
        }
        _ => Vec::new(),
    };
    QueryValue::Json(serde_json::Value::Array(
        keys.into_iter().map(serde_json::Value::String).collect(),
    ))
}

fn evaluate_reference(
    reference: &Reference,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    match reference {
        Reference::Alias(alias) => {
            if let Some(value) = binding.values.get(alias) {
                return Ok(value.clone());
            }
            if let Some(node) = binding.nodes.get(alias) {
                return Ok(QueryValue::Node(node_summary(
                    node,
                    None,
                    degrees,
                    Vec::new(),
                )));
            }
            if let Some(edge) = binding.edges.get(alias) {
                return Ok(QueryValue::Edge(edge_summary(edge)));
            }
            Err(unsupported(&format!("unknown alias {alias}")))
        }
        Reference::Property { alias, path } => {
            if let Some(value) = binding.values.get(alias) {
                return Ok(value_property(value, path));
            }
            if let Some(node) = binding.nodes.get(alias) {
                return Ok(node_property(node, path, degrees));
            }
            if let Some(edge) = binding.edges.get(alias) {
                return Ok(edge_property(edge, path));
            }
            Err(unsupported(&format!("unknown alias {alias}")))
        }
        Reference::EdgeType(alias) => {
            if let Some(QueryValue::Edge(edge)) = binding.values.get(alias) {
                return Ok(QueryValue::String(edge.kind.clone()));
            }
            binding
                .edges
                .get(alias)
                .map(|edge| QueryValue::String(edge.kind.as_str().to_owned()))
                .ok_or_else(|| unsupported(&format!("unknown relationship alias {alias}")))
        }
    }
}

fn value_property(value: &QueryValue, path: &[String]) -> QueryValue {
    let Some((first, rest)) = path.split_first() else {
        return value.clone();
    };
    let json = match value {
        QueryValue::Json(value) => value,
        QueryValue::Node(node) => {
            return match first.as_str() {
                "id" | "node_id" if rest.is_empty() => QueryValue::String(node.id.clone()),
                "name" if rest.is_empty() => QueryValue::String(node.name.clone()),
                "qualified_name" | "qn" if rest.is_empty() => {
                    QueryValue::String(node.qualified_name.clone())
                }
                "label" if rest.is_empty() => QueryValue::String(node.label.clone()),
                "file" | "file_path" if rest.is_empty() => node
                    .file_path
                    .clone()
                    .map_or(QueryValue::Null, QueryValue::String),
                property if rest.is_empty() => node
                    .properties
                    .get(property)
                    .map_or(QueryValue::Null, json_value),
                _ => QueryValue::Null,
            };
        }
        QueryValue::Edge(edge) => {
            return match first.as_str() {
                "source" | "source_id" if rest.is_empty() => {
                    QueryValue::String(edge.source_id.clone())
                }
                "target" | "target_id" if rest.is_empty() => {
                    QueryValue::String(edge.target_id.clone())
                }
                "kind" | "type" if rest.is_empty() => QueryValue::String(edge.kind.clone()),
                "discriminator" if rest.is_empty() => {
                    QueryValue::String(edge.discriminator.clone())
                }
                property if rest.is_empty() => edge
                    .properties
                    .get(property)
                    .map_or(QueryValue::Null, json_value),
                _ => QueryValue::Null,
            };
        }
        _ => return QueryValue::Null,
    };
    let mut current = json;
    for segment in std::iter::once(first).chain(rest) {
        let Some(next) = current.as_object().and_then(|object| object.get(segment)) else {
            return QueryValue::Null;
        };
        current = next;
    }
    json_value(current)
}

fn node_property(
    node: &GraphNode,
    path: &[String],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> QueryValue {
    let Some(property) = path.first().map(String::as_str) else {
        return QueryValue::Null;
    };
    let (in_degree, out_degree) = degrees.get(&node.id).copied().unwrap_or((0, 0));
    let fixed = match property {
        "id" | "node_id" => Some(QueryValue::String(node.id.as_str().to_owned())),
        "project" | "project_id" => Some(QueryValue::String(node.project.as_str().to_owned())),
        "label" => Some(QueryValue::String(node.label.as_str().to_owned())),
        "name" => Some(QueryValue::String(node.name.clone())),
        "qualified_name" | "qn" => {
            Some(QueryValue::String(node.qualified_name.as_str().to_owned()))
        }
        "file" | "file_path" => Some(node.file_path.as_ref().map_or(QueryValue::Null, |path| {
            QueryValue::String(path.as_str().to_owned())
        })),
        "start_byte" => Some(optional_u64(node.source_span.map(|span| span.bytes.start))),
        "end_byte" => Some(optional_u64(node.source_span.map(|span| span.bytes.end))),
        "start_line" => Some(optional_u64(
            node.source_span.map(|span| span.start.row + 1),
        )),
        "end_line" => Some(optional_u64(node.source_span.map(|span| span.end.row + 1))),
        "generation" => Some(unsigned_value(node.generation.value())),
        "in_degree" => Some(unsigned_value(u64::try_from(in_degree).unwrap_or(u64::MAX))),
        "out_degree" => Some(unsigned_value(
            u64::try_from(out_degree).unwrap_or(u64::MAX),
        )),
        "degree" => Some(unsigned_value(
            u64::try_from(in_degree.saturating_add(out_degree)).unwrap_or(u64::MAX),
        )),
        _ => None,
    };
    if path.len() == 1 {
        return fixed.unwrap_or_else(|| {
            node.properties
                .get(property)
                .map_or(QueryValue::Null, json_value)
        });
    }
    if property == "properties" {
        return json_path(&node.properties, &path[1..]);
    }
    QueryValue::Null
}

fn edge_property(edge: &GraphEdge, path: &[String]) -> QueryValue {
    let Some(property) = path.first().map(String::as_str) else {
        return QueryValue::Null;
    };
    let fixed = match property {
        "source" | "source_id" => Some(QueryValue::String(edge.source.as_str().to_owned())),
        "target" | "target_id" => Some(QueryValue::String(edge.target.as_str().to_owned())),
        "kind" | "type" => Some(QueryValue::String(edge.kind.as_str().to_owned())),
        "discriminator" => Some(QueryValue::String(edge.discriminator.as_str().to_owned())),
        "generation" => Some(unsigned_value(edge.generation.value())),
        _ => None,
    };
    if path.len() == 1 {
        return fixed.unwrap_or_else(|| {
            edge.properties
                .get(property)
                .map_or(QueryValue::Null, json_value)
        });
    }
    if property == "properties" {
        return json_path(&edge.properties, &path[1..]);
    }
    QueryValue::Null
}

fn json_path(properties: &BTreeMap<String, serde_json::Value>, path: &[String]) -> QueryValue {
    let Some((first, rest)) = path.split_first() else {
        return QueryValue::Json(serde_json::to_value(properties).unwrap_or_default());
    };
    let Some(mut value) = properties.get(first) else {
        return QueryValue::Null;
    };
    for segment in rest {
        let Some(next) = value.as_object().and_then(|object| object.get(segment)) else {
            return QueryValue::Null;
        };
        value = next;
    }
    json_value(value)
}

fn json_value(value: &serde_json::Value) -> QueryValue {
    match value {
        serde_json::Value::Null => QueryValue::Null,
        serde_json::Value::Bool(value) => QueryValue::Bool(*value),
        serde_json::Value::Number(value) => value.as_i64().map_or_else(
            || {
                value.as_u64().map_or_else(
                    || value.as_f64().map_or(QueryValue::Null, QueryValue::Float),
                    QueryValue::Unsigned,
                )
            },
            QueryValue::Integer,
        ),
        serde_json::Value::String(value) => QueryValue::String(value.clone()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            QueryValue::Json(value.clone())
        }
    }
}

fn query_value_to_json(value: QueryValue) -> Result<serde_json::Value, QueryError> {
    Ok(match value {
        QueryValue::Null => serde_json::Value::Null,
        QueryValue::Bool(value) => serde_json::Value::Bool(value),
        QueryValue::Integer(value) => serde_json::Value::Number(value.into()),
        QueryValue::Unsigned(value) => serde_json::Value::Number(value.into()),
        QueryValue::Float(value) => serde_json::Number::from_f64(value)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        QueryValue::String(value) => serde_json::Value::String(value),
        QueryValue::Json(value) => value,
        QueryValue::Node(value) => serde_json::to_value(value)
            .map_err(|error| unsupported(&format!("cannot serialize node value: {error}")))?,
        QueryValue::Edge(value) => serde_json::to_value(value)
            .map_err(|error| unsupported(&format!("cannot serialize edge value: {error}")))?,
    })
}

fn optional_u64(value: Option<u64>) -> QueryValue {
    value.map_or(QueryValue::Null, unsigned_value)
}

fn unsigned_value(value: u64) -> QueryValue {
    i64::try_from(value).map_or(QueryValue::Unsigned(value), QueryValue::Integer)
}

fn edge_summary(edge: &GraphEdge) -> EdgeSummary {
    EdgeSummary {
        source_id: edge.source.as_str().to_owned(),
        target_id: edge.target.as_str().to_owned(),
        kind: edge.kind.as_str().to_owned(),
        discriminator: edge.discriminator.as_str().to_owned(),
        generation: edge.generation.value(),
        properties: edge.properties.clone(),
    }
}

fn values_equal(left: &QueryValue, right: &QueryValue) -> bool {
    compare_numeric(left, right).map_or_else(|| left == right, Ordering::is_eq)
}

fn compare_values(left: &QueryValue, right: &QueryValue) -> Ordering {
    if let Some(ordering) = compare_numeric(left, right) {
        return ordering;
    }
    match (left, right) {
        (QueryValue::String(left), QueryValue::String(right)) => left.cmp(right),
        (QueryValue::Bool(left), QueryValue::Bool(right)) => left.cmp(right),
        _ => row_key(std::slice::from_ref(left)).cmp(&row_key(std::slice::from_ref(right))),
    }
}

fn compare_numeric(left: &QueryValue, right: &QueryValue) -> Option<Ordering> {
    if let QueryValue::String(value) = left
        && is_numeric_value(right)
    {
        return parse_numeric_value(value).and_then(|value| compare_numeric(&value, right));
    }
    if let QueryValue::String(value) = right
        && is_numeric_value(left)
    {
        return parse_numeric_value(value).and_then(|value| compare_numeric(left, &value));
    }
    Some(match (left, right) {
        (QueryValue::Integer(left), QueryValue::Integer(right)) => left.cmp(right),
        (QueryValue::Unsigned(left), QueryValue::Unsigned(right)) => left.cmp(right),
        (QueryValue::Float(left), QueryValue::Float(right)) => left.total_cmp(right),
        (QueryValue::Integer(left), QueryValue::Unsigned(right)) => compare_i64_u64(*left, *right),
        (QueryValue::Unsigned(left), QueryValue::Integer(right)) => {
            compare_i64_u64(*right, *left).reverse()
        }
        (QueryValue::Integer(left), QueryValue::Float(right)) => compare_i64_f64(*left, *right),
        (QueryValue::Float(left), QueryValue::Integer(right)) => {
            compare_i64_f64(*right, *left).reverse()
        }
        (QueryValue::Unsigned(left), QueryValue::Float(right)) => compare_u64_f64(*left, *right),
        (QueryValue::Float(left), QueryValue::Unsigned(right)) => {
            compare_u64_f64(*right, *left).reverse()
        }
        _ => return None,
    })
}

const fn is_numeric_value(value: &QueryValue) -> bool {
    matches!(
        value,
        QueryValue::Integer(_) | QueryValue::Unsigned(_) | QueryValue::Float(_)
    )
}

fn parse_numeric_value(value: &str) -> Option<QueryValue> {
    if value.contains('.') || value.contains('e') || value.contains('E') {
        return value.parse().ok().map(QueryValue::Float);
    }
    value
        .parse::<i64>()
        .map(QueryValue::Integer)
        .ok()
        .or_else(|| value.parse::<u64>().map(QueryValue::Unsigned).ok())
}

fn compare_i64_u64(signed: i64, unsigned: u64) -> Ordering {
    u64::try_from(signed).map_or(Ordering::Less, |signed| signed.cmp(&unsigned))
}

fn compare_i64_f64(integer: i64, float: f64) -> Ordering {
    if float.is_nan() {
        return Ordering::Less;
    }
    if float.is_sign_negative() && float.to_bits() & i64::MAX.cast_unsigned() != 0 {
        if integer >= 0 {
            Ordering::Greater
        } else {
            compare_positive_u64_f64(integer.unsigned_abs(), -float).reverse()
        }
    } else if integer < 0 {
        Ordering::Less
    } else {
        compare_positive_u64_f64(integer.unsigned_abs(), float)
    }
}

fn compare_u64_f64(integer: u64, float: f64) -> Ordering {
    if float.is_nan() {
        return Ordering::Less;
    }
    if float.is_sign_negative() && float.to_bits() & i64::MAX.cast_unsigned() != 0 {
        Ordering::Greater
    } else {
        compare_positive_u64_f64(integer, float)
    }
}

fn compare_positive_u64_f64(integer: u64, float: f64) -> Ordering {
    let bits = float.to_bits();
    let exponent_bits = u16::try_from((bits >> 52) & 0x7ff).expect("f64 exponent fits u16");
    if exponent_bits == 0x7ff {
        return Ordering::Less;
    }
    if bits & i64::MAX.cast_unsigned() == 0 {
        return integer.cmp(&0);
    }
    if exponent_bits == 0 {
        return if integer == 0 {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }

    let exponent = i32::from(exponent_bits) - 1023;
    if exponent < 0 {
        return if integer == 0 {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    if exponent >= 64 {
        return Ordering::Less;
    }

    let mantissa = (bits & ((1_u64 << 52) - 1)) | (1_u64 << 52);
    let (whole, has_fraction) = if exponent >= 52 {
        let shift = u32::try_from(exponent - 52).expect("nonnegative f64 shift");
        (mantissa << shift, false)
    } else {
        let shift = u32::try_from(52 - exponent).expect("positive f64 shift");
        let fraction_mask = (1_u64 << shift) - 1;
        (mantissa >> shift, mantissa & fraction_mask != 0)
    };
    integer.cmp(&whole).then_with(|| {
        if has_fraction {
            Ordering::Less
        } else {
            Ordering::Equal
        }
    })
}

fn string_pair(
    left: &QueryValue,
    right: &QueryValue,
    predicate: impl Fn(&str, &str) -> bool,
) -> bool {
    match (left, right) {
        (QueryValue::String(left), QueryValue::String(right)) => predicate(left, right),
        _ => false,
    }
}

fn execute_with_clause<'a>(
    clause: &WithClause,
    bindings: Vec<Binding<'a>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    let has_aggregate = clause.projections.iter().any(|projection| {
        matches!(
            projection.expression,
            ProjectionExpression::Aggregate { .. }
        )
    });
    let mut projected = Vec::new();
    if has_aggregate {
        let grouping: Vec<&ProjectionExpression> = clause
            .projections
            .iter()
            .filter_map(|projection| match &projection.expression {
                ProjectionExpression::Aggregate { .. } => None,
                expression => Some(expression),
            })
            .collect();
        let mut groups: BTreeMap<String, Vec<Binding<'a>>> = BTreeMap::new();
        for binding in bindings {
            let key = grouping
                .iter()
                .map(|expression| evaluate_projection_expression(expression, &binding, degrees))
                .collect::<Result<Vec<_>, _>>()?;
            groups.entry(row_key(&key)).or_default().push(binding);
        }
        if groups.is_empty() && grouping.is_empty() {
            groups.insert(String::new(), Vec::new());
        }
        for group in groups.into_values() {
            let first = group.first();
            let values = clause
                .projections
                .iter()
                .map(|projection| match &projection.expression {
                    ProjectionExpression::Aggregate {
                        kind,
                        target,
                        distinct,
                    } => evaluate_aggregate(*kind, target.as_ref(), *distinct, &group, degrees),
                    expression => first.map_or_else(
                        || Ok(QueryValue::Null),
                        |binding| evaluate_projection_expression(expression, binding, degrees),
                    ),
                })
                .collect::<Result<Vec<_>, _>>()?;
            projected.push((
                binding_from_with(&clause.projections, &values, first),
                values,
            ));
        }
    } else {
        for binding in bindings {
            let values = clause
                .projections
                .iter()
                .map(|projection| {
                    evaluate_projection_expression(&projection.expression, &binding, degrees)
                })
                .collect::<Result<Vec<_>, _>>()?;
            projected.push((
                binding_from_with(&clause.projections, &values, Some(&binding)),
                values,
            ));
        }
    }
    if clause.distinct {
        let mut seen = BTreeSet::new();
        projected.retain(|(_, values)| seen.insert(row_key(values)));
    }
    if let Some(filter) = &clause.filter {
        let mut retained = Vec::with_capacity(projected.len());
        for (binding, values) in projected {
            if evaluate_expression(filter, &binding, degrees)? {
                retained.push((binding, values));
            }
        }
        projected = retained;
    }
    let mut ordered = projected
        .into_iter()
        .map(|(binding, values)| {
            let order = clause
                .order
                .iter()
                .map(|order| evaluate_reference(&order.reference, &binding, degrees))
                .collect::<Result<Vec<_>, _>>()?;
            Ok::<_, QueryError>((order, values, binding))
        })
        .collect::<Result<Vec<_>, _>>()?;
    ordered.sort_by(
        |(left_order, left_values, _), (right_order, right_values, _)| {
            for (index, order) in clause.order.iter().enumerate() {
                let mut ordering = compare_values(&left_order[index], &right_order[index]);
                if order.descending {
                    ordering = ordering.reverse();
                }
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            row_key(left_values).cmp(&row_key(right_values))
        },
    );
    let skipped = clause.skip.min(ordered.len());
    ordered.drain(..skipped);
    if let Some(limit) = clause.limit {
        ordered.truncate(limit);
    }
    if ordered.len() > MAX_INTERMEDIATE_BINDINGS {
        return Err(unsupported("WITH exceeds intermediate binding safety cap"));
    }
    Ok(ordered.into_iter().map(|(_, _, binding)| binding).collect())
}

fn binding_from_with<'a>(
    projections: &[Projection],
    values: &[QueryValue],
    source: Option<&Binding<'a>>,
) -> Binding<'a> {
    let mut binding = Binding::default();
    if let Some(source) = source {
        binding.all_nodes = source.all_nodes;
        binding.all_edges = source.all_edges;
    }
    for (projection, value) in projections.iter().zip(values) {
        if let ProjectionExpression::Reference(Reference::Alias(alias)) = &projection.expression
            && let Some(source) = source
        {
            if let Some(node) = source.nodes.get(alias) {
                binding.nodes.insert(projection.column.clone(), node);
                continue;
            }
            if let Some(edge) = source.edges.get(alias) {
                binding.edges.insert(projection.column.clone(), edge);
                continue;
            }
        }
        binding
            .values
            .insert(projection.column.clone(), value.clone());
    }
    binding
}

fn materialize_rows(
    query: &ParsedQuery,
    bindings: Vec<Binding<'_>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Vec<QueryValue>>, QueryError> {
    let has_aggregate = query.projections.iter().any(|projection| {
        matches!(
            projection.expression,
            ProjectionExpression::Aggregate { .. }
        )
    });
    let mut rows = if has_aggregate {
        materialize_aggregate_rows(query, bindings, degrees)?
    } else {
        bindings
            .into_iter()
            .map(|binding| {
                let values = query
                    .projections
                    .iter()
                    .map(|projection| {
                        evaluate_projection_expression(&projection.expression, &binding, degrees)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let order_values =
                    materialize_order_values(query, &values, Some(&binding), degrees)?;
                Ok::<_, QueryError>((order_values, values))
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    rows.sort_by(|(left_order, left_row), (right_order, right_row)| {
        for (index, clause) in query.order.iter().enumerate() {
            let mut ordering = compare_values(&left_order[index], &right_order[index]);
            if clause.descending {
                ordering = ordering.reverse();
            }
            if ordering != Ordering::Equal {
                return ordering;
            }
        }
        row_key(left_row).cmp(&row_key(right_row))
    });
    Ok(rows.into_iter().map(|(_, values)| values).collect())
}

fn evaluate_projection_expression(
    expression: &ProjectionExpression,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    match expression {
        ProjectionExpression::Reference(reference) => {
            evaluate_reference(reference, binding, degrees)
        }
        ProjectionExpression::Function { name, arguments } => {
            evaluate_scalar_function(name, arguments, binding, degrees)
        }
        ProjectionExpression::Case(expression) => {
            evaluate_case_expression(expression, binding, degrees)
        }
        ProjectionExpression::Aggregate { .. } => {
            Err(unsupported("aggregate requires grouped evaluation"))
        }
    }
}

fn evaluate_case_expression(
    expression: &CaseExpression,
    binding: &Binding<'_>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    let subject = expression
        .subject
        .as_ref()
        .map(|subject| evaluate_operand(subject, binding, degrees))
        .transpose()?;
    for branch in &expression.branches {
        let matches = match &branch.when {
            CaseWhen::Predicate(predicate) => evaluate_expression(predicate, binding, degrees)?,
            CaseWhen::Value(value) => {
                let expected = subject
                    .as_ref()
                    .ok_or_else(|| unsupported("simple CASE is missing its subject"))?;
                values_equal(expected, &evaluate_operand(value, binding, degrees)?)
            }
        };
        if matches {
            return evaluate_operand(&branch.then, binding, degrees);
        }
    }
    expression.fallback.as_ref().map_or_else(
        || Ok(QueryValue::Null),
        |fallback| evaluate_operand(fallback, binding, degrees),
    )
}

fn materialize_aggregate_rows<'a>(
    query: &ParsedQuery,
    bindings: Vec<Binding<'a>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<(Vec<QueryValue>, Vec<QueryValue>)>, QueryError> {
    let grouping: Vec<&ProjectionExpression> = query
        .projections
        .iter()
        .filter_map(|projection| match &projection.expression {
            ProjectionExpression::Aggregate { .. } => None,
            expression => Some(expression),
        })
        .collect();
    let mut groups: BTreeMap<String, Vec<Binding<'a>>> = BTreeMap::new();
    for binding in bindings {
        let key_values = grouping
            .iter()
            .map(|expression| evaluate_projection_expression(expression, &binding, degrees))
            .collect::<Result<Vec<_>, _>>()?;
        groups
            .entry(row_key(&key_values))
            .or_default()
            .push(binding);
    }
    if groups.is_empty() && grouping.is_empty() {
        groups.insert(String::new(), Vec::new());
    }

    groups
        .into_values()
        .map(|group| {
            let first = group.first();
            let values = query
                .projections
                .iter()
                .map(|projection| match &projection.expression {
                    ProjectionExpression::Aggregate {
                        kind,
                        target,
                        distinct,
                    } => evaluate_aggregate(*kind, target.as_ref(), *distinct, &group, degrees),
                    expression => first.map_or_else(
                        || Ok(QueryValue::Null),
                        |binding| evaluate_projection_expression(expression, binding, degrees),
                    ),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let order_values = materialize_order_values(query, &values, first, degrees)?;
            Ok((order_values, values))
        })
        .collect()
}

fn materialize_order_values(
    query: &ParsedQuery,
    values: &[QueryValue],
    binding: Option<&Binding<'_>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<QueryValue>, QueryError> {
    query
        .order
        .iter()
        .map(|clause| {
            if let Reference::Alias(alias) = &clause.reference
                && let Some(index) = query
                    .projections
                    .iter()
                    .position(|projection| projection.column == *alias)
            {
                return Ok(values[index].clone());
            }
            binding.map_or_else(
                || Ok(QueryValue::Null),
                |binding| evaluate_reference(&clause.reference, binding, degrees),
            )
        })
        .collect()
}

fn evaluate_aggregate(
    kind: AggregateKind,
    target: Option<&Reference>,
    distinct: bool,
    bindings: &[Binding<'_>],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryValue, QueryError> {
    let mut values = if let Some(reference) = target {
        bindings
            .iter()
            .map(|binding| evaluate_reference(reference, binding, degrees))
            .filter_map(|value| match value {
                Ok(QueryValue::Null) => None,
                other => Some(other),
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![QueryValue::Bool(true); bindings.len()]
    };
    if distinct {
        let mut seen = BTreeSet::new();
        values.retain(|value| seen.insert(row_key(std::slice::from_ref(value))));
    }
    match kind {
        AggregateKind::Count => i64::try_from(values.len())
            .map(QueryValue::Integer)
            .map_err(|_| unsupported("aggregate count exceeds signed integer range")),
        AggregateKind::Sum => aggregate_sum(&values),
        AggregateKind::Average => {
            if values.is_empty() {
                return Ok(QueryValue::Null);
            }
            let sum = values.iter().try_fold(0.0, |sum, value| {
                Ok::<_, QueryError>(sum + numeric_value(value)?)
            })?;
            Ok(QueryValue::Float(sum / values.len() as f64))
        }
        AggregateKind::Minimum => Ok(values
            .into_iter()
            .min_by(compare_values)
            .unwrap_or(QueryValue::Null)),
        AggregateKind::Maximum => Ok(values
            .into_iter()
            .max_by(compare_values)
            .unwrap_or(QueryValue::Null)),
        AggregateKind::Collect => Ok(QueryValue::Json(serde_json::Value::Array(
            values
                .into_iter()
                .map(query_value_to_json)
                .collect::<Result<_, _>>()?,
        ))),
    }
}

fn aggregate_sum(values: &[QueryValue]) -> Result<QueryValue, QueryError> {
    let all_integral = values
        .iter()
        .all(|value| matches!(value, QueryValue::Integer(_) | QueryValue::Unsigned(_)));
    if all_integral {
        let sum = values.iter().try_fold(0_i128, |sum, value| {
            let value = match value {
                QueryValue::Integer(value) => i128::from(*value),
                QueryValue::Unsigned(value) => i128::from(*value),
                _ => unreachable!(),
            };
            sum.checked_add(value)
                .ok_or_else(|| unsupported("SUM exceeds numeric range"))
        })?;
        if let Ok(value) = i64::try_from(sum) {
            return Ok(QueryValue::Integer(value));
        }
        if let Ok(value) = u64::try_from(sum) {
            return Ok(QueryValue::Unsigned(value));
        }
        return Err(unsupported("SUM exceeds numeric range"));
    }
    values
        .iter()
        .try_fold(0.0, |sum, value| {
            Ok::<_, QueryError>(sum + numeric_value(value)?)
        })
        .map(QueryValue::Float)
}

fn numeric_value(value: &QueryValue) -> Result<f64, QueryError> {
    match value {
        QueryValue::Integer(value) => Ok(*value as f64),
        QueryValue::Unsigned(value) => Ok(*value as f64),
        QueryValue::Float(value) => Ok(*value),
        _ => Err(unsupported("numeric aggregate requires numeric values")),
    }
}

fn row_key(row: &[QueryValue]) -> String {
    serde_json::to_string(row).unwrap_or_else(|_| format!("{row:?}"))
}

struct BoundedRows {
    rows: Vec<Vec<QueryValue>>,
    total: usize,
    truncated: bool,
    #[cfg(test)]
    peak_retained: usize,
}

fn collect_bounded_rows(
    rows: impl IntoIterator<Item = Result<Vec<QueryValue>, QueryError>>,
    skip: usize,
    limit: usize,
) -> Result<BoundedRows, QueryError> {
    let retain_limit = skip.saturating_add(limit);
    let mut retained: BTreeMap<String, Vec<Vec<QueryValue>>> = BTreeMap::new();
    let mut retained_count = 0_usize;
    let mut total_before_skip = 0_usize;
    #[cfg(test)]
    let mut peak_retained = 0_usize;

    for row in rows {
        let row = row?;
        total_before_skip = total_before_skip.saturating_add(1);
        if retain_limit == 0 {
            continue;
        }
        retained.entry(row_key(&row)).or_default().push(row);
        retained_count += 1;
        if retained_count > retain_limit {
            let greatest_key = retained
                .last_key_value()
                .map(|(key, _)| key.clone())
                .expect("a retained row has a greatest key");
            let remove_bucket = {
                let bucket = retained
                    .get_mut(&greatest_key)
                    .expect("greatest row bucket exists");
                bucket.pop();
                bucket.is_empty()
            };
            if remove_bucket {
                retained.remove(&greatest_key);
            }
            retained_count -= 1;
        }
        #[cfg(test)]
        {
            peak_retained = peak_retained.max(retained_count);
        }
    }

    let skipped = skip.min(total_before_skip);
    let total = total_before_skip - skipped;
    let mut rows = retained
        .into_values()
        .flatten()
        .collect::<Vec<Vec<QueryValue>>>();
    rows.drain(..skipped.min(rows.len()));
    rows.truncate(limit);
    Ok(BoundedRows {
        rows,
        total,
        truncated: total > limit,
        #[cfg(test)]
        peak_retained,
    })
}

#[cfg(test)]
mod tests {
    use super::{collect_bounded_rows, row_key};
    use crate::types::QueryValue;

    #[test]
    fn simple_limit_retains_only_bounded_rows_without_changing_result_metadata() {
        let input = (0..100)
            .rev()
            .map(|value| Ok(vec![QueryValue::Integer(value)]))
            .collect::<Vec<_>>();
        let mut expected = (0..100)
            .rev()
            .map(|value| vec![QueryValue::Integer(value)])
            .collect::<Vec<_>>();
        expected.sort_by_key(|row| row_key(row));
        expected.drain(..1);
        expected.truncate(3);

        let bounded = collect_bounded_rows(input, 1, 3).expect("bounded rows");

        assert_eq!(bounded.rows, expected);
        assert_eq!(bounded.total, 99);
        assert!(bounded.truncated);
        assert!(bounded.peak_retained <= 4);
    }
}
