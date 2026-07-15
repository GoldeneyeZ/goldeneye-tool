use std::collections::BTreeMap;

use goldeneye_domain::GraphNode;
use serde_json::Value;

use super::model::{Endpoint, ProjectGraph};

pub(super) fn route_handler_registry(projects: &[ProjectGraph]) -> BTreeMap<String, Vec<Endpoint>> {
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

pub(super) fn channel_listener_registry(
    projects: &[ProjectGraph],
) -> BTreeMap<String, Vec<Endpoint>> {
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

pub(super) fn node_map(nodes: &[GraphNode]) -> BTreeMap<&str, &GraphNode> {
    nodes.iter().map(|node| (node.id.as_str(), node)).collect()
}

pub(super) fn matching_handlers<'a>(
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

pub(super) fn channel_key(name: &str, transport: &str) -> String {
    format!("{}\0{}", transport.to_ascii_lowercase(), name)
}
