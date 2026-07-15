use std::collections::BTreeMap;

use goldeneye_domain::{EdgeDiscriminator, EdgeKind, GraphEdge, GraphIdentityError, NodeId};
use serde_json::{Value, json};

use super::model::{Endpoint, ProjectGraph};
use super::{CrossLinkError, MAX_CROSS_EDGES_PER_PROJECT};

pub(super) fn endpoint_properties(edge: &GraphEdge, handler: &Endpoint, route_qn: &str) -> Value {
    let mut properties = edge.properties.clone();
    properties.insert("target_project".to_owned(), json!(handler.project));
    properties.insert("target_name".to_owned(), json!(handler.handler_name));
    properties.insert("target_file".to_owned(), json!(handler.handler_file));
    properties.insert("route_qn".to_owned(), json!(route_qn));
    properties.insert("direction".to_owned(), json!("forward"));
    Value::Object(properties.into_iter().collect())
}

pub(super) fn cross_edge(
    project: &ProjectGraph,
    source: &NodeId,
    target: &NodeId,
    kind: &str,
    target_project: &str,
    properties: Value,
) -> Result<GraphEdge, GraphIdentityError> {
    let mut edge = GraphEdge::new(
        project.record.id.clone(),
        source.clone(),
        target.clone(),
        EdgeKind::new(kind)?,
        project.record.generation,
    );
    edge.discriminator = EdgeDiscriminator::new(target_project)?;
    edge.properties = match properties {
        Value::Object(properties) => properties.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    Ok(edge)
}

pub(super) fn push_bounded(
    edges: &mut BTreeMap<String, Vec<GraphEdge>>,
    project: &str,
    edge: GraphEdge,
) -> Result<(), CrossLinkError> {
    let project_edges = edges
        .get_mut(project)
        .expect("derived edge map includes every project");
    if project_edges.len() >= MAX_CROSS_EDGES_PER_PROJECT {
        return Err(CrossLinkError::EdgeLimit {
            project: project.to_owned(),
            limit: MAX_CROSS_EDGES_PER_PROJECT,
        });
    }
    project_edges.push(edge);
    Ok(())
}

pub(super) fn deduplicate_edges(edges: &mut Vec<GraphEdge>) {
    edges.sort_by(compare_edge_identity);
    edges.dedup_by(|left, right| compare_edge_identity(left, right).is_eq());
}

fn compare_edge_identity(left: &GraphEdge, right: &GraphEdge) -> std::cmp::Ordering {
    (&left.source, &left.target, &left.kind, &left.discriminator).cmp(&(
        &right.source,
        &right.target,
        &right.kind,
        &right.discriminator,
    ))
}
