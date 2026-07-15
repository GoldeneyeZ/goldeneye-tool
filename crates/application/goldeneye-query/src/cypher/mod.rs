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

use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};

mod ast;
mod evaluate;
mod fast_path;
mod lexer;
mod parser;
mod projection;

use ast::{MatchPattern, Operand, ParsedQuery, ProjectionExpression, Reference};
use evaluate::{Binding, execute_match_clauses, execute_unwind};
use fast_path::execute_simple_node_limit;
use lexer::{Token, lex, reject_mutations, split_union_tokens};
use parser::Parser;
use projection::{
    collect_bounded_rows, evaluate_projection_expression, execute_with_clause,
    expand_star_projections, expand_with_star_projections, materialize_rows,
};

use crate::types::{QueryError, QueryGraphRequest, QueryGraphResult, QueryValue};

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
    let (mut branches, union_all) = parse_query_branches(request)?;
    if union_all.is_empty() {
        let tokens = branches.pop().expect("query has one branch");
        return execute_branch(request, tokens, nodes, edges, degrees, request.max_rows);
    }
    execute_union(request, branches, union_all, nodes, edges, degrees)
}

fn parse_query_branches(
    request: &QueryGraphRequest,
) -> Result<(Vec<Vec<Token>>, Vec<bool>), QueryError> {
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
    Ok((branches, union_all))
}

fn execute_branch(
    request: &QueryGraphRequest,
    tokens: Vec<Token>,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    row_cap: usize,
) -> Result<QueryGraphResult, QueryError> {
    let query = Parser::new(tokens, request.query.len()).parse()?;
    execute_parsed(request, query, nodes, edges, degrees, row_cap)
}

fn execute_union(
    request: &QueryGraphRequest,
    branches: Vec<Vec<Token>>,
    union_all: Vec<bool>,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<QueryGraphResult, QueryError> {
    let results = branches
        .into_iter()
        .map(|tokens| execute_branch(request, tokens, nodes, edges, degrees, MAX_QUERY_ROWS))
        .collect::<Result<Vec<_>, _>>()?;
    let columns = results
        .first()
        .ok_or_else(|| unsupported("UNION requires at least one query"))?
        .columns
        .clone();
    if results.iter().any(|result| result.columns != columns) {
        return Err(unsupported("UNION branches must return identical columns"));
    }
    let (rows, total, truncated, warning) =
        merge_union_results(results, union_all, request.max_rows);
    Ok(QueryGraphResult {
        project: request.project.as_str().to_owned(),
        columns,
        rows,
        total,
        truncated,
        warning,
    })
}

fn merge_union_results(
    mut results: Vec<QueryGraphResult>,
    union_all: Vec<bool>,
    row_cap: usize,
) -> (Vec<Vec<QueryValue>>, usize, bool, Option<String>) {
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
    let truncated = source_truncated || total > row_cap;
    rows.truncate(row_cap);
    let warning = (!warnings.is_empty()).then(|| warnings.join("; "));
    (rows, total, truncated, warning)
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
    let bindings = query_bindings(&query, nodes, edges, degrees)?;
    expand_projections(&mut query);
    let columns = query
        .projections
        .iter()
        .map(|projection| projection.column.clone())
        .collect();
    let (rows, total, truncated) = project_rows(&query, bindings, degrees, row_cap)?;
    Ok(QueryGraphResult {
        project: request.project.as_str().to_owned(),
        columns,
        rows,
        total,
        truncated,
        warning: (!query.warnings.is_empty()).then(|| query.warnings.join("; ")),
    })
}

fn query_bindings<'a>(
    query: &ParsedQuery,
    nodes: &'a [GraphNode],
    edges: &'a [GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<Binding<'a>>, QueryError> {
    let initial = execute_unwind(query.unwind.as_ref(), degrees)?;
    let mut bindings = execute_match_clauses(&query.matches, initial, nodes, edges, degrees)?;
    if let Some(with_clause) = &query.with_clause {
        bindings = execute_with_clause(with_clause, bindings, degrees)?;
    }
    Ok(bindings)
}

fn expand_projections(query: &mut ParsedQuery) {
    if query.star {
        query.projections = query.with_clause.as_ref().map_or_else(
            || expand_star_projections(&query.matches),
            expand_with_star_projections,
        );
    }
}

fn project_rows(
    query: &ParsedQuery,
    bindings: Vec<Binding<'_>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    row_cap: usize,
) -> Result<(Vec<Vec<QueryValue>>, usize, bool), QueryError> {
    let materialized_limit = row_cap.min(query.limit.unwrap_or(usize::MAX));
    let has_aggregate = query.projections.iter().any(|projection| {
        matches!(
            projection.expression,
            ProjectionExpression::Aggregate { .. }
        )
    });
    if !has_aggregate && !query.distinct && query.order.is_empty() {
        stream_projected_rows(query, bindings, degrees, materialized_limit)
    } else {
        materialized_projected_rows(query, bindings, degrees, materialized_limit)
    }
}

fn stream_projected_rows(
    query: &ParsedQuery,
    bindings: Vec<Binding<'_>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    limit: usize,
) -> Result<(Vec<Vec<QueryValue>>, usize, bool), QueryError> {
    let projected = bindings.into_iter().map(|binding| {
        query
            .projections
            .iter()
            .map(|projection| {
                evaluate_projection_expression(&projection.expression, &binding, degrees)
            })
            .collect::<Result<Vec<_>, _>>()
    });
    let bounded = collect_bounded_rows(projected, query.skip, limit)?;
    Ok((bounded.rows, bounded.total, bounded.truncated))
}

fn materialized_projected_rows(
    query: &ParsedQuery,
    bindings: Vec<Binding<'_>>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    limit: usize,
) -> Result<(Vec<Vec<QueryValue>>, usize, bool), QueryError> {
    let mut rows = materialize_rows(query, bindings, degrees)?;
    if query.distinct {
        let mut seen = BTreeSet::new();
        rows.retain(|row| seen.insert(row_key(row)));
    }
    let skipped = query.skip.min(rows.len());
    rows.drain(..skipped);
    let total = rows.len();
    let truncated = total > limit;
    rows.truncate(limit);
    Ok((rows, total, truncated))
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

fn row_key(row: &[QueryValue]) -> String {
    serde_json::to_string(row).unwrap_or_else(|_| format!("{row:?}"))
}
