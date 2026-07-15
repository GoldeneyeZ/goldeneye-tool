use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use goldeneye_domain::NodeId;

use super::super::{
    MAX_INTERMEDIATE_BINDINGS,
    ast::{Projection, ProjectionExpression, Reference, WithClause},
    evaluate::{Binding, compare_values, evaluate_expression, evaluate_reference},
    row_key, unsupported,
};
use super::{aggregate::evaluate_aggregate, rows::evaluate_projection_expression};
use crate::types::{QueryError, QueryValue};

pub(in crate::cypher) fn execute_with_clause<'a>(
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
