use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{
    EdgeDiscriminator, EdgeKind, GraphEdge, GraphIdentityError, GraphNode, NodeId, ProjectRecord,
};
use goldeneye_store::{Store, StoreError};
use serde_json::{Value, json};
use thiserror::Error;

const MAX_CROSS_EDGES_PER_PROJECT: usize = 100_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CrossLinkOutcome {
    pub projects: usize,
    pub edges: usize,
}

#[derive(Debug, Error)]
pub enum CrossLinkError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Identity(#[from] GraphIdentityError),
    #[error("cross-project edge limit exceeded for {project}: {limit}")]
    EdgeLimit { project: String, limit: usize },
}

/// Rebuilds all derived cross-project call and channel edges.
///
/// Local call edges remain untouched. Forward and reverse cross edges carry the remote project
/// plus compact endpoint metadata, while targeting nodes inside their own project database.
///
/// # Errors
///
/// Returns graph read, identity, edge-limit, or atomic replacement errors.
#[allow(clippy::too_many_lines)]
pub fn rebuild(store: &mut Store) -> Result<CrossLinkOutcome, CrossLinkError> {
    let records = store.list_projects()?;
    if records.len() <= 1 {
        if let Some(record) = records.first() {
            store.replace_cross_project_edges(&record.id, &[])?;
        }
        return Ok(CrossLinkOutcome {
            projects: records.len(),
            edges: 0,
        });
    }
    let mut projects = Vec::with_capacity(records.len());
    for record in records {
        projects.push(ProjectGraph {
            nodes: store.list_nodes(&record.id)?,
            edges: store.list_edges(&record.id)?,
            record,
        });
    }

    let route_handlers = route_handler_registry(&projects);
    let channel_listeners = channel_listener_registry(&projects);
    let mut derived = projects
        .iter()
        .map(|project| (project.record.id.as_str().to_owned(), Vec::new()))
        .collect::<BTreeMap<_, Vec<GraphEdge>>>();

    for source in &projects {
        let nodes = node_map(&source.nodes);
        for edge in &source.edges {
            if let Some(cross_kind) = cross_call_kind(edge.kind.as_str()) {
                let Some(route) = nodes.get(edge.target.as_str()) else {
                    continue;
                };
                let Some(caller) = nodes.get(edge.source.as_str()) else {
                    continue;
                };
                let mut seen_projects = BTreeSet::new();
                for handler in matching_handlers(&route_handlers, route.qualified_name.as_str()) {
                    if handler.project == source.record.id.as_str()
                        || !seen_projects.insert(handler.project.clone())
                    {
                        continue;
                    }
                    let forward = cross_edge(
                        source,
                        &edge.source,
                        &edge.target,
                        cross_kind,
                        &handler.project,
                        endpoint_properties(edge, handler, route.qualified_name.as_str()),
                    )?;
                    push_bounded(&mut derived, source.record.id.as_str(), forward)?;

                    let Some(target) = project_by_name(&projects, &handler.project) else {
                        continue;
                    };
                    let reverse_properties = json!({
                        "target_project": source.record.id.as_str(),
                        "target_name": caller.name,
                        "target_file": caller.file_path.as_ref().map_or("", |path| path.as_str()),
                        "route_qn": route.qualified_name.as_str(),
                        "direction": "reverse",
                    });
                    let reverse = cross_edge(
                        target,
                        &handler.handler_id,
                        &handler.route_id,
                        cross_kind,
                        source.record.id.as_str(),
                        reverse_properties,
                    )?;
                    push_bounded(&mut derived, &handler.project, reverse)?;
                }
            }

            if edge.kind.as_str() == "EMITS" {
                let Some(channel) = nodes.get(edge.target.as_str()) else {
                    continue;
                };
                if channel.label.as_str() != "Channel" {
                    continue;
                }
                let transport = channel
                    .properties
                    .get("transport")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let key = channel_key(&channel.name, transport);
                let mut seen_projects = BTreeSet::new();
                for listener in channel_listeners.get(&key).into_iter().flatten() {
                    if listener.project == source.record.id.as_str()
                        || !seen_projects.insert(listener.project.clone())
                    {
                        continue;
                    }
                    let forward = cross_edge(
                        source,
                        &edge.source,
                        &edge.target,
                        "CROSS_CHANNEL",
                        &listener.project,
                        json!({
                            "target_project": listener.project,
                            "target_name": listener.handler_name,
                            "target_file": listener.handler_file,
                            "channel": channel.name,
                            "transport": transport,
                            "direction": "forward",
                        }),
                    )?;
                    push_bounded(&mut derived, source.record.id.as_str(), forward)?;

                    let Some(target) = project_by_name(&projects, &listener.project) else {
                        continue;
                    };
                    let emitter = nodes.get(edge.source.as_str());
                    let reverse = cross_edge(
                        target,
                        &listener.handler_id,
                        &listener.route_id,
                        "CROSS_CHANNEL",
                        source.record.id.as_str(),
                        json!({
                            "target_project": source.record.id.as_str(),
                            "target_name": emitter.map_or("", |node| node.name.as_str()),
                            "target_file": emitter.and_then(|node| node.file_path.as_ref()).map_or("", |path| path.as_str()),
                            "channel": channel.name,
                            "transport": transport,
                            "direction": "reverse",
                        }),
                    )?;
                    push_bounded(&mut derived, &listener.project, reverse)?;
                }
            }
        }
    }

    let mut outcome = CrossLinkOutcome {
        projects: projects.len(),
        edges: 0,
    };
    for project in &projects {
        let mut edges = derived
            .remove(project.record.id.as_str())
            .unwrap_or_default();
        deduplicate_edges(&mut edges);
        outcome.edges += store.replace_cross_project_edges(&project.record.id, &edges)?;
    }
    Ok(outcome)
}

struct ProjectGraph {
    record: ProjectRecord,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone)]
struct Endpoint {
    project: String,
    route_id: NodeId,
    handler_id: NodeId,
    handler_name: String,
    handler_file: String,
}

fn route_handler_registry(projects: &[ProjectGraph]) -> BTreeMap<String, Vec<Endpoint>> {
    let mut registry = BTreeMap::<String, Vec<Endpoint>>::new();
    for project in projects {
        let nodes = node_map(&project.nodes);
        for edge in &project.edges {
            if edge.kind.as_str() != "HANDLES" {
                continue;
            }
            let (Some(handler), Some(route)) = (
                nodes.get(edge.source.as_str()),
                nodes.get(edge.target.as_str()),
            ) else {
                continue;
            };
            if route.label.as_str() != "Route" {
                continue;
            }
            registry
                .entry(route.qualified_name.as_str().to_owned())
                .or_default()
                .push(Endpoint {
                    project: project.record.id.as_str().to_owned(),
                    route_id: route.id.clone(),
                    handler_id: handler.id.clone(),
                    handler_name: handler.name.clone(),
                    handler_file: handler
                        .file_path
                        .as_ref()
                        .map_or_else(String::new, |path| path.as_str().to_owned()),
                });
        }
    }
    for endpoints in registry.values_mut() {
        endpoints.sort_by(|left, right| {
            (&left.project, &left.handler_id).cmp(&(&right.project, &right.handler_id))
        });
    }
    registry
}

fn channel_listener_registry(projects: &[ProjectGraph]) -> BTreeMap<String, Vec<Endpoint>> {
    let mut registry = BTreeMap::<String, Vec<Endpoint>>::new();
    for project in projects {
        let nodes = node_map(&project.nodes);
        for edge in &project.edges {
            if !matches!(edge.kind.as_str(), "LISTENS" | "CONSUMES" | "HANDLES") {
                continue;
            }
            let (Some(handler), Some(channel)) = (
                nodes.get(edge.source.as_str()),
                nodes.get(edge.target.as_str()),
            ) else {
                continue;
            };
            if channel.label.as_str() != "Channel" {
                continue;
            }
            let transport = channel
                .properties
                .get("transport")
                .and_then(Value::as_str)
                .unwrap_or_default();
            registry
                .entry(channel_key(&channel.name, transport))
                .or_default()
                .push(Endpoint {
                    project: project.record.id.as_str().to_owned(),
                    route_id: channel.id.clone(),
                    handler_id: handler.id.clone(),
                    handler_name: handler.name.clone(),
                    handler_file: handler
                        .file_path
                        .as_ref()
                        .map_or_else(String::new, |path| path.as_str().to_owned()),
                });
        }
    }
    registry
}

fn node_map(nodes: &[GraphNode]) -> BTreeMap<&str, &GraphNode> {
    nodes.iter().map(|node| (node.id.as_str(), node)).collect()
}

fn matching_handlers<'a>(
    registry: &'a BTreeMap<String, Vec<Endpoint>>,
    route_qn: &str,
) -> Vec<&'a Endpoint> {
    let mut matches = Vec::new();
    if let Some(exact) = registry.get(route_qn) {
        matches.extend(exact);
    }
    let Some((method, path)) = split_route_qn(route_qn) else {
        return matches;
    };
    let any = format!("__route__ANY__{path}");
    if method != "ANY"
        && let Some(endpoints) = registry.get(&any)
    {
        matches.extend(endpoints);
    }
    for (candidate, endpoints) in registry {
        if candidate == route_qn || candidate == &any {
            continue;
        }
        let Some((candidate_method, candidate_path)) = split_route_qn(candidate) else {
            continue;
        };
        if (candidate_method == method || candidate_method == "ANY")
            && route_template_matches(candidate_path, path)
        {
            matches.extend(endpoints);
        }
    }
    matches
}

fn split_route_qn(value: &str) -> Option<(&str, &str)> {
    value.strip_prefix("__route__")?.split_once("__")
}

fn route_template_matches(template: &str, concrete: &str) -> bool {
    let template = template.split('/').collect::<Vec<_>>();
    let concrete = concrete.split('/').collect::<Vec<_>>();
    template.len() == concrete.len()
        && template
            .iter()
            .zip(concrete)
            .all(|(expected, actual)| *expected == "{}" || *expected == actual)
}

const fn cross_call_kind(kind: &str) -> Option<&'static str> {
    match kind.as_bytes() {
        b"HTTP_CALLS" => Some("CROSS_HTTP_CALLS"),
        b"ASYNC_CALLS" => Some("CROSS_ASYNC_CALLS"),
        b"GRAPHQL_CALLS" => Some("CROSS_GRAPHQL_CALLS"),
        b"GRPC_CALLS" => Some("CROSS_GRPC_CALLS"),
        b"TRPC_CALLS" => Some("CROSS_TRPC_CALLS"),
        _ => None,
    }
}

fn endpoint_properties(edge: &GraphEdge, handler: &Endpoint, route_qn: &str) -> Value {
    let mut properties = edge.properties.clone();
    properties.insert("target_project".to_owned(), json!(handler.project));
    properties.insert("target_name".to_owned(), json!(handler.handler_name));
    properties.insert("target_file".to_owned(), json!(handler.handler_file));
    properties.insert("route_qn".to_owned(), json!(route_qn));
    properties.insert("direction".to_owned(), json!("forward"));
    Value::Object(properties.into_iter().collect())
}

fn cross_edge(
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

fn push_bounded(
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

fn deduplicate_edges(edges: &mut Vec<GraphEdge>) {
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

fn channel_key(name: &str, transport: &str) -> String {
    format!("{}\0{}", transport.to_ascii_lowercase(), name)
}

fn project_by_name<'a>(projects: &'a [ProjectGraph], name: &str) -> Option<&'a ProjectGraph> {
    projects
        .iter()
        .find(|project| project.record.id.as_str() == name)
}

#[cfg(test)]
mod tests {
    use goldeneye_domain::{
        EdgeDiscriminator, EdgeKind, Generation, GraphEdge, GraphNode, NodeId, NodeLabel,
        ProjectId, ProjectRecord, QualifiedName,
    };
    use goldeneye_store::Store;

    use super::{deduplicate_edges, rebuild};

    #[test]
    fn single_project_rebuild_clears_stale_cross_edges_without_loading_graphs() {
        let mut store = Store::open_in_memory().expect("store");
        let project_id = ProjectId::new("api").expect("project ID");
        let project = ProjectRecord::new(project_id.clone(), "/api").expect("project");
        let source = GraphNode::new(
            project_id.clone(),
            NodeId::new("source").expect("source ID"),
            NodeLabel::new("Function").expect("source label"),
            "source",
            QualifiedName::new("api.source").expect("source qualified name"),
            None,
            None,
            Generation::new(0),
        )
        .expect("source node");
        let target = GraphNode::new(
            project_id.clone(),
            NodeId::new("target").expect("target ID"),
            NodeLabel::new("Function").expect("target label"),
            "target",
            QualifiedName::new("api.target").expect("target qualified name"),
            None,
            None,
            Generation::new(0),
        )
        .expect("target node");
        let replacement = store
            .replace_project_graph(&project, vec![], vec![source, target], vec![])
            .expect("replace project graph");
        let stale = GraphEdge::new(
            project_id.clone(),
            NodeId::new("source").expect("source ID"),
            NodeId::new("target").expect("target ID"),
            EdgeKind::new("CROSS_HTTP_CALLS").expect("cross edge kind"),
            replacement.generation,
        );
        store
            .replace_cross_project_edges(&project_id, &[stale])
            .expect("seed stale cross edge");

        let outcome = rebuild(&mut store).expect("rebuild");

        assert_eq!(outcome.projects, 1);
        assert_eq!(outcome.edges, 0);
        assert!(
            store
                .list_edges(&project_id)
                .expect("list edges")
                .is_empty()
        );
    }

    #[test]
    fn duplicate_cross_edges_are_collapsed_by_identity() {
        let project = ProjectId::new("api").expect("project");
        let source = NodeId::new("handler").expect("source");
        let target = NodeId::new("route").expect("target");
        let kind = EdgeKind::new("CROSS_HTTP_CALLS").expect("kind");
        let mut first = GraphEdge::new(
            project.clone(),
            source.clone(),
            target.clone(),
            kind.clone(),
            Generation::new(1),
        );
        first.discriminator = EdgeDiscriminator::new("client").expect("discriminator");
        let mut second = GraphEdge::new(project, source, target, kind, Generation::new(1));
        second.discriminator = EdgeDiscriminator::new("client").expect("discriminator");
        second
            .properties
            .insert("target_name".to_owned(), "other_caller".into());
        let mut edges = vec![first, second];

        deduplicate_edges(&mut edges);

        assert_eq!(edges.len(), 1);
    }
}
