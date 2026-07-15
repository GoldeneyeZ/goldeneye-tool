use std::{cmp::Ordering, collections::BTreeMap};

use goldeneye_domain::NodeId;
use regex::Regex;

use super::super::{
    MAX_INTERMEDIATE_BINDINGS,
    ast::{Expression, MatchPattern, Operand, Predicate, PredicateOperator},
    unsupported,
};
use super::{
    binding::{Binding, merge_bindings},
    compare::{compare_values, string_pair, values_equal},
    path::build_bindings_bounded,
    scalar::evaluate_operand,
};
use crate::types::{QueryError, QueryValue};

pub(in crate::cypher) fn evaluate_expression(
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
