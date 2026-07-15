use std::collections::{BTreeMap, BTreeSet};

use goldeneye_domain::{GraphEdge, GraphNode, ProjectRecord};
use goldeneye_ports::CrossLinkRepository;
use serde_json::{Value, json};

use super::edges::{cross_edge, deduplicate_edges, endpoint_properties, push_bounded};
use super::model::{Endpoint, ProjectGraph};
use super::registry::{
    channel_key, channel_listener_registry, matching_handlers, node_map, route_handler_registry,
};
use super::{CrossLinkError, CrossLinkOutcome};

type DerivedEdges = BTreeMap<String, Vec<GraphEdge>>;
type EndpointRegistry = BTreeMap<String, Vec<Endpoint>>;

/// Rebuilds all derived cross-project call and channel edges.
///
/// Local call edges remain untouched. Forward and reverse cross edges carry the remote project
/// plus compact endpoint metadata, while targeting nodes inside their own project database.
///
/// # Errors
///
/// Returns graph read, identity, edge-limit, or atomic replacement errors.
pub fn rebuild(store: &mut impl CrossLinkRepository) -> Result<CrossLinkOutcome, CrossLinkError> {
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

    let projects = load_project_graphs(store, records)?;
    let derived = derive_edges(&projects)?;
    persist_derived_edges(store, &projects, derived)
}

fn load_project_graphs(
    store: &impl CrossLinkRepository,
    records: Vec<ProjectRecord>,
) -> Result<Vec<ProjectGraph>, CrossLinkError> {
    let mut projects = Vec::with_capacity(records.len());
    for record in records {
        projects.push(ProjectGraph {
            nodes: store.list_nodes(&record.id)?,
            edges: store.list_edges(&record.id)?,
            record,
        });
    }
    Ok(projects)
}

fn derive_edges(projects: &[ProjectGraph]) -> Result<DerivedEdges, CrossLinkError> {
    let route_handlers = route_handler_registry(projects);
    let channel_listeners = channel_listener_registry(projects);
    let mut derived = projects
        .iter()
        .map(|project| (project.record.id.as_str().to_owned(), Vec::new()))
        .collect::<DerivedEdges>();

    for source in projects {
        derive_project_edges(
            source,
            projects,
            &route_handlers,
            &channel_listeners,
            &mut derived,
        )?;
    }
    Ok(derived)
}

fn derive_project_edges(
    source: &ProjectGraph,
    projects: &[ProjectGraph],
    route_handlers: &EndpointRegistry,
    channel_listeners: &EndpointRegistry,
    derived: &mut DerivedEdges,
) -> Result<(), CrossLinkError> {
    let nodes = node_map(&source.nodes);
    for edge in &source.edges {
        if derive_call_edges(source, projects, route_handlers, derived, &nodes, edge)? {
            continue;
        }
        derive_channel_edges(source, projects, channel_listeners, derived, &nodes, edge)?;
    }
    Ok(())
}

struct CallContext<'a> {
    source: &'a ProjectGraph,
    edge: &'a GraphEdge,
    route: &'a GraphNode,
    caller: &'a GraphNode,
    cross_kind: &'static str,
}

fn derive_call_edges(
    source: &ProjectGraph,
    projects: &[ProjectGraph],
    route_handlers: &EndpointRegistry,
    derived: &mut DerivedEdges,
    nodes: &BTreeMap<&str, &GraphNode>,
    edge: &GraphEdge,
) -> Result<bool, CrossLinkError> {
    let Some(cross_kind) = cross_call_kind(edge.kind.as_str()) else {
        return Ok(false);
    };
    let Some(route) = nodes.get(edge.target.as_str()) else {
        return Ok(true);
    };
    let Some(caller) = nodes.get(edge.source.as_str()) else {
        return Ok(true);
    };
    let context = CallContext {
        source,
        edge,
        route,
        caller,
        cross_kind,
    };
    let mut seen_projects = BTreeSet::new();
    for handler in matching_handlers(route_handlers, route.qualified_name.as_str()) {
        if handler.project == source.record.id.as_str()
            || !seen_projects.insert(handler.project.clone())
        {
            continue;
        }
        append_call_pair(&context, projects, derived, handler)?;
    }
    Ok(false)
}

fn append_call_pair(
    context: &CallContext<'_>,
    projects: &[ProjectGraph],
    derived: &mut DerivedEdges,
    handler: &Endpoint,
) -> Result<(), CrossLinkError> {
    let forward = cross_edge(
        context.source,
        &context.edge.source,
        &context.edge.target,
        context.cross_kind,
        &handler.project,
        endpoint_properties(context.edge, handler, context.route.qualified_name.as_str()),
    )?;
    push_bounded(derived, context.source.record.id.as_str(), forward)?;

    let Some(target) = project_by_name(projects, &handler.project) else {
        return Ok(());
    };
    let reverse_properties = json!({
        "target_project": context.source.record.id.as_str(),
        "target_name": context.caller.name,
        "target_file": context.caller.file_path.as_ref().map_or("", |path| path.as_str()),
        "route_qn": context.route.qualified_name.as_str(),
        "direction": "reverse",
    });
    let reverse = cross_edge(
        target,
        &handler.handler_id,
        &handler.route_id,
        context.cross_kind,
        context.source.record.id.as_str(),
        reverse_properties,
    )?;
    push_bounded(derived, &handler.project, reverse)
}

struct ChannelContext<'a> {
    source: &'a ProjectGraph,
    edge: &'a GraphEdge,
    channel: &'a GraphNode,
    transport: &'a str,
    nodes: &'a BTreeMap<&'a str, &'a GraphNode>,
}

fn derive_channel_edges(
    source: &ProjectGraph,
    projects: &[ProjectGraph],
    channel_listeners: &EndpointRegistry,
    derived: &mut DerivedEdges,
    nodes: &BTreeMap<&str, &GraphNode>,
    edge: &GraphEdge,
) -> Result<(), CrossLinkError> {
    if edge.kind.as_str() != "EMITS" {
        return Ok(());
    }
    let Some(channel) = nodes.get(edge.target.as_str()) else {
        return Ok(());
    };
    if channel.label.as_str() != "Channel" {
        return Ok(());
    }
    let transport = channel
        .properties
        .get("transport")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let key = channel_key(&channel.name, transport);
    let context = ChannelContext {
        source,
        edge,
        channel,
        transport,
        nodes,
    };
    let mut seen_projects = BTreeSet::new();
    for listener in channel_listeners.get(&key).into_iter().flatten() {
        if listener.project == source.record.id.as_str()
            || !seen_projects.insert(listener.project.clone())
        {
            continue;
        }
        append_channel_pair(&context, projects, derived, listener)?;
    }
    Ok(())
}

fn append_channel_pair(
    context: &ChannelContext<'_>,
    projects: &[ProjectGraph],
    derived: &mut DerivedEdges,
    listener: &Endpoint,
) -> Result<(), CrossLinkError> {
    let forward = cross_edge(
        context.source,
        &context.edge.source,
        &context.edge.target,
        "CROSS_CHANNEL",
        &listener.project,
        json!({
            "target_project": listener.project,
            "target_name": listener.handler_name,
            "target_file": listener.handler_file,
            "channel": context.channel.name,
            "transport": context.transport,
            "direction": "forward",
        }),
    )?;
    push_bounded(derived, context.source.record.id.as_str(), forward)?;

    let Some(target) = project_by_name(projects, &listener.project) else {
        return Ok(());
    };
    let emitter = context.nodes.get(context.edge.source.as_str());
    let reverse = cross_edge(
        target,
        &listener.handler_id,
        &listener.route_id,
        "CROSS_CHANNEL",
        context.source.record.id.as_str(),
        json!({
            "target_project": context.source.record.id.as_str(),
            "target_name": emitter.map_or("", |node| node.name.as_str()),
            "target_file": emitter.and_then(|node| node.file_path.as_ref()).map_or("", |path| path.as_str()),
            "channel": context.channel.name,
            "transport": context.transport,
            "direction": "reverse",
        }),
    )?;
    push_bounded(derived, &listener.project, reverse)
}

fn persist_derived_edges(
    store: &mut impl CrossLinkRepository,
    projects: &[ProjectGraph],
    mut derived: DerivedEdges,
) -> Result<CrossLinkOutcome, CrossLinkError> {
    let mut outcome = CrossLinkOutcome {
        projects: projects.len(),
        edges: 0,
    };
    for project in projects {
        let mut edges = derived
            .remove(project.record.id.as_str())
            .unwrap_or_default();
        deduplicate_edges(&mut edges);
        outcome.edges += store.replace_cross_project_edges(&project.record.id, &edges)?;
    }
    Ok(outcome)
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

fn project_by_name<'a>(projects: &'a [ProjectGraph], name: &str) -> Option<&'a ProjectGraph> {
    projects
        .iter()
        .find(|project| project.record.id.as_str() == name)
}
