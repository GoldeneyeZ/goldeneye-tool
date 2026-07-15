use std::{cmp::Ordering, collections::BTreeMap};

use goldeneye_domain::NodeId;

use super::super::{
    ast::{CaseExpression, CaseWhen, ParsedQuery, ProjectionExpression},
    evaluate::{
        Binding, compare_values, evaluate_expression, evaluate_operand, evaluate_reference,
        evaluate_scalar_function, values_equal,
    },
    row_key, unsupported,
};
use super::aggregate::{materialize_aggregate_rows, materialize_order_values};
use crate::types::{QueryError, QueryValue};

pub(in crate::cypher) fn materialize_rows(
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

pub(in crate::cypher) fn evaluate_projection_expression(
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

pub(in crate::cypher) struct BoundedRows {
    pub(in crate::cypher) rows: Vec<Vec<QueryValue>>,
    pub(in crate::cypher) total: usize,
    pub(in crate::cypher) truncated: bool,
    #[cfg(test)]
    peak_retained: usize,
}

pub(in crate::cypher) fn collect_bounded_rows(
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
