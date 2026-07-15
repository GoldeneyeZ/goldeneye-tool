use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use goldeneye_domain::{
    EdgeKind, Generation, GraphEdge, GraphNode, GraphProperties, NodeId, NodeLabel, ProjectId,
    ProjectRelativePath, QualifiedName,
};
use goldeneye_ports::{
    IndexExtractedCall as ExtractedCall, IndexExtractedImport as ExtractedImport,
};
use serde_json::{Value, json};

use crate::IndexError;

const MAX_SYNTHETIC_NODES: usize = 8_192;
const MAX_SYNTHETIC_EDGES: usize = 32_768;
const MAX_LITERAL_BYTES: usize = 512;

mod configuration;
mod data_flow;
mod environment;
mod libraries;
mod packages;
mod protocols;
mod routes;
mod services;

use configuration::create_config_links;
use data_flow::create_data_flows;
use environment::create_environment_edges;
use packages::create_package_links;
use protocols::create_protocol_handlers;
use routes::create_decorator_routes;
use services::create_service_edges;

#[derive(Clone)]
pub(crate) struct SourceFile {
    pub path: ProjectRelativePath,
    pub source: Arc<[u8]>,
}

pub(crate) fn apply_project(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    calls: &[ExtractedCall],
    imports: &[ExtractedImport],
    sources: &[SourceFile],
) -> Result<(), IndexError> {
    let import_map = import_map(imports);
    create_environment_edges(project, nodes, edges, calls)?;
    create_service_edges(project, nodes, edges, calls, &import_map)?;
    create_decorator_routes(project, nodes, edges, sources)?;
    create_protocol_handlers(project, nodes, edges, sources)?;
    create_config_links(project, nodes, edges)?;
    create_package_links(project, nodes, edges, imports, sources)?;
    create_data_flows(project, nodes, edges)?;
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort_by(|left, right| {
        (
            &left.source,
            left.kind.as_str(),
            &left.target,
            left.discriminator.as_str(),
        )
            .cmp(&(
                &right.source,
                right.kind.as_str(),
                &right.target,
                right.discriminator.as_str(),
            ))
    });
    Ok(())
}

fn ensure_node(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    label: &str,
    name: &str,
    qualified_name: &str,
    file_path: Option<ProjectRelativePath>,
    properties: GraphProperties,
) -> Result<NodeId, IndexError> {
    if let Some(node) = nodes
        .iter()
        .find(|node| node.qualified_name.as_str() == qualified_name)
    {
        return Ok(node.id.clone());
    }
    let synthetic_count = nodes
        .iter()
        .filter(|node| matches!(node.label.as_str(), "Route" | "EnvVar" | "Package"))
        .count();
    if synthetic_count >= MAX_SYNTHETIC_NODES {
        return Err(IndexError::CoordinateOverflow("synthetic node bound"));
    }
    let id = stable_node_id(label, qualified_name)?;
    let node = GraphNode::new(
        project.clone(),
        id.clone(),
        NodeLabel::new(label)?,
        name,
        QualifiedName::new(qualified_name)?,
        file_path,
        None,
        Generation::new(0),
    )?
    .with_properties(properties);
    nodes.push(node);
    Ok(id)
}

fn push_edge(
    project: &ProjectId,
    edges: &mut Vec<GraphEdge>,
    source: &NodeId,
    target: &NodeId,
    kind: &str,
    properties: GraphProperties,
) -> Result<(), IndexError> {
    let synthetic_count = edges
        .iter()
        .filter(|edge| {
            matches!(
                edge.kind.as_str(),
                "HTTP_CALLS" | "ASYNC_CALLS" | "HANDLES" | "DATA_FLOWS" | "CONFIGURES"
            )
        })
        .count();
    if synthetic_count >= MAX_SYNTHETIC_EDGES {
        return Err(IndexError::CoordinateOverflow("synthetic edge bound"));
    }
    if edges
        .iter()
        .any(|edge| &edge.source == source && &edge.target == target && edge.kind.as_str() == kind)
    {
        return Ok(());
    }
    edges.push(
        GraphEdge::new(
            project.clone(),
            source.clone(),
            target.clone(),
            EdgeKind::new(kind)?,
            Generation::new(0),
        )
        .with_properties(properties),
    );
    Ok(())
}

fn stable_node_id(label: &str, qualified_name: &str) -> Result<NodeId, IndexError> {
    let hash = blake3::hash(format!("goldeneye-node-v1\0{label}\0{qualified_name}").as_bytes());
    Ok(NodeId::new(format!(
        "{}:{}",
        label.to_ascii_lowercase(),
        &hash.to_hex()[..32]
    ))?)
}

fn json_properties<const N: usize>(entries: [(&str, Value); N]) -> GraphProperties {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn import_map(imports: &[ExtractedImport]) -> BTreeMap<(ProjectRelativePath, String), String> {
    imports
        .iter()
        .map(|import| {
            (
                (import.file.clone(), import.alias.clone()),
                import.module_path.clone(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        configuration::normalize_config_key,
        routes::{canonical_route_path, first_string_literal, route_method_from_annotation},
    };

    #[test]
    fn route_paths_canonicalize_framework_parameters() {
        assert_eq!(
            canonical_route_path("/users/:id/posts/{post}"),
            "/users/{}/posts/{}"
        );
        assert_eq!(canonical_route_path("/files/<path:name>"), "/files/{}");
        assert_eq!(canonical_route_path("/jobs/${jobId}"), "/jobs/{}");
    }

    #[test]
    fn literal_and_decorator_helpers_are_bounded_and_deterministic() {
        assert_eq!(
            first_string_literal("client.get(\"/v1/users\")").as_deref(),
            Some("/v1/users")
        );
        assert_eq!(
            route_method_from_annotation("@router.post('/v1/users')"),
            Some("POST")
        );
        assert_eq!(
            normalize_config_key("database.maxConnections"),
            ["database", "max", "connections"]
        );
    }
}
