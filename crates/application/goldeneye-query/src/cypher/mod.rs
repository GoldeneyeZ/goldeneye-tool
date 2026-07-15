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

mod ast;
mod evaluate;
mod lexer;
mod parser;

use ast::{
    AggregateKind, CaseExpression, CaseWhen, MatchClause, MatchPattern, Operand, ParsedQuery,
    Projection, ProjectionExpression, Reference, WithClause,
};
use evaluate::{
    Binding, compare_values, evaluate_expression, evaluate_operand, evaluate_reference,
    evaluate_scalar_function, execute_match_clauses, execute_unwind, node_matches,
    query_value_to_json, values_equal,
};
use lexer::{lex, reject_mutations, split_union_tokens};
use parser::Parser;

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
