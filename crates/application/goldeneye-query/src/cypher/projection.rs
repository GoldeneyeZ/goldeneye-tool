use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use goldeneye_domain::NodeId;

use super::{
    MAX_INTERMEDIATE_BINDINGS,
    ast::{
        AggregateKind, CaseExpression, CaseWhen, MatchClause, ParsedQuery, Projection,
        ProjectionExpression, Reference, WithClause,
    },
    evaluate::{
        Binding, compare_values, evaluate_expression, evaluate_operand, evaluate_reference,
        evaluate_scalar_function, query_value_to_json, values_equal,
    },
    pattern_node_aliases, reference_column, row_key, unsupported,
};
use crate::types::{QueryError, QueryValue};

pub(super) fn expand_star_projections(matches: &[MatchClause]) -> Vec<Projection> {
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

pub(super) fn expand_with_star_projections(clause: &WithClause) -> Vec<Projection> {
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

pub(super) fn execute_with_clause<'a>(
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

pub(super) fn materialize_rows(
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

pub(super) fn evaluate_projection_expression(
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

pub(super) struct BoundedRows {
    pub(super) rows: Vec<Vec<QueryValue>>,
    pub(super) total: usize,
    pub(super) truncated: bool,
    #[cfg(test)]
    peak_retained: usize,
}

pub(super) fn collect_bounded_rows(
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
