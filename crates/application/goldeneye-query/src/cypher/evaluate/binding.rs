use std::collections::BTreeMap;

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};

use super::super::{
    MAX_INTERMEDIATE_BINDINGS,
    ast::{MatchClause, MatchPattern, UnwindClause},
    pattern_node_aliases, unsupported,
};
use super::{
    compare::values_equal, path::build_bindings_bounded, predicate::evaluate_expression,
    reference::json_value, scalar::evaluate_operand,
};
use crate::types::{QueryError, QueryValue};

#[derive(Clone, Default)]
pub(in crate::cypher) struct Binding<'a> {
    pub(in crate::cypher) nodes: BTreeMap<String, &'a GraphNode>,
    pub(in crate::cypher) edges: BTreeMap<String, &'a GraphEdge>,
    pub(in crate::cypher) values: BTreeMap<String, QueryValue>,
    pub(in crate::cypher) all_nodes: Option<&'a [GraphNode]>,
    pub(in crate::cypher) all_edges: Option<&'a [GraphEdge]>,
}

pub(in crate::cypher) fn execute_unwind<'a>(
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

pub(in crate::cypher) fn execute_match_clauses<'a>(
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

pub(super) fn merge_bindings<'a>(
    base: &Binding<'a>,
    candidate: &Binding<'a>,
) -> Option<Binding<'a>> {
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
