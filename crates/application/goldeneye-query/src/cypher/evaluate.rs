use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};
use regex::Regex;

use super::{
    MAX_INTERMEDIATE_BINDINGS,
    ast::{
        EdgeDirection, EdgeMatch, EdgePattern, Expression, MatchClause, MatchPattern, NodePattern,
        Operand, Predicate, PredicateOperator, Reference, UnwindClause,
    },
    pattern_node_aliases, row_key, unsupported,
};
use crate::{
    engine::node_summary,
    types::{EdgeSummary, QueryError, QueryValue},
};

#[derive(Clone, Default)]
pub(super) struct Binding<'a> {
    pub(super) nodes: BTreeMap<String, &'a GraphNode>,
    pub(super) edges: BTreeMap<String, &'a GraphEdge>,
    pub(super) values: BTreeMap<String, QueryValue>,
    pub(super) all_nodes: Option<&'a [GraphNode]>,
    pub(super) all_edges: Option<&'a [GraphEdge]>,
}

pub(super) fn execute_unwind<'a>(
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

pub(super) fn execute_match_clauses<'a>(
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

pub(super) fn node_matches(
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

pub(super) fn evaluate_expression(
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

pub(super) fn evaluate_operand(
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

pub(super) fn evaluate_scalar_function(
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

pub(super) fn evaluate_reference(
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

pub(super) fn query_value_to_json(value: QueryValue) -> Result<serde_json::Value, QueryError> {
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

pub(super) fn values_equal(left: &QueryValue, right: &QueryValue) -> bool {
    compare_numeric(left, right).map_or_else(|| left == right, Ordering::is_eq)
}

pub(super) fn compare_values(left: &QueryValue, right: &QueryValue) -> Ordering {
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
