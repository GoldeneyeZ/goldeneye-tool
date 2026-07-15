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
mod lexer;
mod parser;
mod projection;

use ast::{MatchPattern, Operand, ParsedQuery, Projection, ProjectionExpression, Reference};
use evaluate::{Binding, evaluate_expression, execute_match_clauses, execute_unwind, node_matches};
use lexer::{lex, reject_mutations, split_union_tokens};
use parser::Parser;
use projection::{
    collect_bounded_rows, evaluate_projection_expression, execute_with_clause,
    expand_star_projections, expand_with_star_projections, materialize_rows,
};

use crate::{
    engine::node_summary,
    types::{QueryError, QueryGraphRequest, QueryGraphResult, QueryValue},
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
