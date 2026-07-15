use super::{
    BTreeMap, BTreeSet, GraphEdge, GraphNode, IndexError, NodeId, ProjectId, json, json_properties,
    push_edge,
};

pub(super) fn create_data_flows(
    project: &ProjectId,
    nodes: &[GraphNode],
    edges: &mut Vec<GraphEdge>,
) -> Result<(), IndexError> {
    let route_ids = nodes
        .iter()
        .filter(|node| node.label.as_str() == "Route")
        .map(|node| node.id.clone())
        .collect::<BTreeSet<_>>();
    let mut callers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    let mut handlers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for edge in edges.iter() {
        if !route_ids.contains(&edge.target) {
            continue;
        }
        match edge.kind.as_str() {
            "HTTP_CALLS" | "ASYNC_CALLS" => callers
                .entry(edge.target.clone())
                .or_default()
                .push(edge.source.clone()),
            "HANDLES" => handlers
                .entry(edge.target.clone())
                .or_default()
                .push(edge.source.clone()),
            _ => {}
        }
    }
    for (route, route_callers) in callers {
        let Some(route_handlers) = handlers.get(&route) else {
            continue;
        };
        for caller in &route_callers {
            for handler in route_handlers {
                if caller == handler {
                    continue;
                }
                push_edge(
                    project,
                    edges,
                    caller,
                    handler,
                    "DATA_FLOWS",
                    json_properties([
                        ("strategy", json!("route_join")),
                        ("route_id", json!(route.as_str())),
                    ]),
                )?;
            }
        }
    }
    Ok(())
}
