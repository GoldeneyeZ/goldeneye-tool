use std::collections::BTreeMap;

use goldeneye_domain::{GraphEdge, GraphNode, NodeId};

use crate::{
    engine::node_summary,
    types::{QueryError, QueryGraphRequest, QueryGraphResult, QueryValue},
};

use super::{
    MAX_INTERMEDIATE_BINDINGS,
    ast::{
        MatchClause, MatchPattern, NodePattern, ParsedQuery, Projection, ProjectionExpression,
        Reference,
    },
    evaluate::{Binding, evaluate_expression, node_matches},
    projection::{collect_bounded_rows, evaluate_projection_expression},
    unsupported,
};

pub(super) fn execute_simple_node_limit(
    request: &QueryGraphRequest,
    query: &ParsedQuery,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    row_cap: usize,
) -> Option<Result<QueryGraphResult, QueryError>> {
    let (query_limit, clause, pattern) = eligibility(query)?;
    let mut candidates = match matching_candidates(nodes, pattern, degrees) {
        Ok(candidates) => candidates,
        Err(error) => return Some(Err(error)),
    };
    let limit = row_cap.min(query_limit);
    if is_qualified_name_projection(query, clause, pattern) {
        return Some(Ok(qualified_name_result(
            request,
            query,
            &mut candidates,
            limit,
        )));
    }
    if is_node_projection(query, clause, pattern) {
        return Some(Ok(node_result(
            request,
            query,
            &mut candidates,
            degrees,
            limit,
        )));
    }
    Some(
        bounded_projection(
            query, clause, pattern, candidates, nodes, edges, degrees, limit,
        )
        .map(|(rows, total, truncated)| result(request, query, rows, total, truncated)),
    )
}

fn eligibility(query: &ParsedQuery) -> Option<(usize, &MatchClause, &NodePattern)> {
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
    Some((query_limit, clause, pattern))
}

fn matching_candidates<'a>(
    nodes: &'a [GraphNode],
    pattern: &NodePattern,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Result<Vec<&'a GraphNode>, QueryError> {
    // Buffer only references so binding-cap errors retain priority over expression errors.
    let mut candidates = Vec::with_capacity(nodes.len().min(MAX_INTERMEDIATE_BINDINGS));
    for node in nodes {
        if node_matches(node, pattern, degrees) {
            if candidates.len() == MAX_INTERMEDIATE_BINDINGS {
                return Err(unsupported("query exceeds intermediate binding safety cap"));
            }
            candidates.push(node);
        }
    }
    Ok(candidates)
}

fn is_qualified_name_projection(
    query: &ParsedQuery,
    clause: &MatchClause,
    pattern: &NodePattern,
) -> bool {
    clause.filter.is_none()
        && matches!(
            query.projections.as_slice(),
            [Projection {
                expression: ProjectionExpression::Reference(Reference::Property { alias, path }),
                ..
            }] if alias == &pattern.alias && path.as_slice() == ["qualified_name"]
        )
}

fn is_node_projection(query: &ParsedQuery, clause: &MatchClause, pattern: &NodePattern) -> bool {
    clause.filter.is_none()
        && matches!(
            query.projections.as_slice(),
            [Projection {
                expression: ProjectionExpression::Reference(Reference::Alias(alias)),
                ..
            }] if alias == &pattern.alias
        )
}

fn qualified_name_result(
    request: &QueryGraphRequest,
    query: &ParsedQuery,
    candidates: &mut Vec<&GraphNode>,
    limit: usize,
) -> QueryGraphResult {
    candidates.sort_by_cached_key(|node| {
        serde_json::to_string(node.qualified_name.as_str())
            .unwrap_or_else(|_| node.qualified_name.as_str().to_owned())
    });
    let skipped = query.skip.min(candidates.len());
    let total = candidates.len() - skipped;
    let rows = candidates
        .iter()
        .skip(skipped)
        .take(limit)
        .map(|node| vec![QueryValue::String(node.qualified_name.as_str().to_owned())])
        .collect();
    result(request, query, rows, total, total > limit)
}

fn node_result(
    request: &QueryGraphRequest,
    query: &ParsedQuery,
    candidates: &mut Vec<&GraphNode>,
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    limit: usize,
) -> QueryGraphResult {
    // QueryValue::Node row keys start with the unique node ID. Sorting its exact JSON encoding
    // preserves collect_bounded_rows ordering without constructing every NodeSummary.
    candidates.sort_by_cached_key(|node| {
        serde_json::to_string(node.id.as_str()).unwrap_or_else(|_| node.id.as_str().to_owned())
    });
    let skipped = query.skip.min(candidates.len());
    let total = candidates.len() - skipped;
    let rows = candidates
        .iter()
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
    result(request, query, rows, total, total > limit)
}

fn bounded_projection(
    query: &ParsedQuery,
    clause: &MatchClause,
    pattern: &NodePattern,
    candidates: Vec<&GraphNode>,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
    limit: usize,
) -> Result<(Vec<Vec<QueryValue>>, usize, bool), QueryError> {
    let projected = candidates
        .into_iter()
        .filter_map(|node| project_candidate(query, clause, pattern, node, nodes, edges, degrees));
    let bounded = collect_bounded_rows(projected, query.skip, limit)?;
    Ok((bounded.rows, bounded.total, bounded.truncated))
}

fn project_candidate(
    query: &ParsedQuery,
    clause: &MatchClause,
    pattern: &NodePattern,
    node: &GraphNode,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    degrees: &BTreeMap<NodeId, (usize, usize)>,
) -> Option<Result<Vec<QueryValue>, QueryError>> {
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
            .collect(),
    )
}

fn result(
    request: &QueryGraphRequest,
    query: &ParsedQuery,
    rows: Vec<Vec<QueryValue>>,
    total: usize,
    truncated: bool,
) -> QueryGraphResult {
    QueryGraphResult {
        project: request.project.as_str().to_owned(),
        columns: query
            .projections
            .iter()
            .map(|item| item.column.clone())
            .collect(),
        rows,
        total,
        truncated,
        warning: (!query.warnings.is_empty()).then(|| query.warnings.join("; ")),
    }
}
