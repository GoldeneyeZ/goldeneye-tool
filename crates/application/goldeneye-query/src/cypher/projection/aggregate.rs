use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::NodeId;

use super::super::{
    ast::{AggregateKind, ParsedQuery, ProjectionExpression, Reference},
    evaluate::{Binding, compare_values, evaluate_reference, query_value_to_json},
    row_key, unsupported,
};
use super::rows::evaluate_projection_expression;
use crate::types::{QueryError, QueryValue};

pub(super) fn materialize_aggregate_rows<'a>(
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

pub(super) fn materialize_order_values(
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

pub(super) fn evaluate_aggregate(
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
