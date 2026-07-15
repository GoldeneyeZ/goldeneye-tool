use std::collections::BTreeMap;

use goldeneye_domain::{GraphEdge, GraphNode, ProjectId};
use goldeneye_store::QueryStore;
use serde_json::{Map, Value, json};

use super::{
    ApiError, ApiRequest, ApiResponse, DEFAULT_LAYOUT_NODES, GoldeneyeBackend, MAX_LAYOUT_NODES,
    internal, project_id,
};

type TargetLayout = (BTreeMap<String, usize>, Value);

impl GoldeneyeBackend {
    pub(super) fn handle_layout(&self, request: &ApiRequest) -> Result<ApiResponse, ApiError> {
        let (requested_project, project_name, max_nodes) = layout_parameters(request)?;
        let store = self.query_store()?;
        let project = project_id(&project_name)?;
        if store.get_project(&project).map_err(internal)?.is_none() {
            return Err(ApiError::new(404, "project not found"));
        }
        let mut body = layout_value(&store, &project, max_nodes)?;
        if project_name == requested_project {
            append_linked_layouts(&store, &project, requested_project, max_nodes, &mut body)?;
        }
        Ok(ApiResponse::ok(body))
    }
}

fn layout_parameters(request: &ApiRequest) -> Result<(&str, String, usize), ApiError> {
    let requested_project = request.required_query("project")?;
    let project_name = if request
        .query
        .get("graph")
        .is_some_and(|value| value == "missed")
    {
        format!("{requested_project}::missed")
    } else {
        requested_project.to_owned()
    };
    let max_nodes = request
        .query
        .get("max_nodes")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| ApiError::new(400, "invalid max_nodes parameter"))?
        .unwrap_or(DEFAULT_LAYOUT_NODES)
        .clamp(1, MAX_LAYOUT_NODES);
    Ok((requested_project, project_name, max_nodes))
}

fn append_linked_layouts(
    store: &QueryStore,
    project: &ProjectId,
    requested_project: &str,
    max_nodes: usize,
    body: &mut Value,
) -> Result<(), ApiError> {
    let linked_projects = linked_projects_value(store, project, max_nodes)?;
    if !linked_projects.is_empty()
        && let Some(object) = body.as_object_mut()
    {
        object.insert("linked_projects".to_owned(), Value::Array(linked_projects));
    }
    let missed = project_id(&format!("{requested_project}::missed"))?;
    if store.get_project(&missed).map_err(internal)?.is_some()
        && let Some(object) = body.as_object_mut()
    {
        object.insert(
            "missed_graph".to_owned(),
            layout_value(store, &missed, max_nodes)?,
        );
    }
    Ok(())
}

fn layout_value(
    store: &QueryStore,
    project: &ProjectId,
    max_nodes: usize,
) -> Result<Value, ApiError> {
    let mut nodes = store.list_nodes(project).map_err(internal)?;
    let edges = store.list_edges(project).map_err(internal)?;
    let total_nodes = nodes.len();
    nodes.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    nodes.truncate(max_nodes);
    let ids = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str().to_owned(), index))
        .collect::<BTreeMap<_, _>>();
    let (degree, inbound_calls, layout_edges) = layout_edges(&edges, &ids, nodes.len());
    let count = usize_to_f64(nodes.len().max(1));
    let layout_nodes = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let (x, y, z) = coordinates(index, count);
            node_value(node, index, x, y, z, degree[index], inbound_calls[index])
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "nodes": layout_nodes,
        "edges": layout_edges,
        "total_nodes": total_nodes,
    }))
}

fn layout_edges(
    edges: &[GraphEdge],
    ids: &BTreeMap<String, usize>,
    node_count: usize,
) -> (Vec<u64>, Vec<u64>, Vec<Value>) {
    let mut degree = vec![0_u64; node_count];
    let mut inbound_calls = vec![0_u64; node_count];
    let mut layout_edges = Vec::new();
    for edge in edges {
        let (Some(&source), Some(&target)) =
            (ids.get(edge.source.as_str()), ids.get(edge.target.as_str()))
        else {
            continue;
        };
        degree[source] += 1;
        degree[target] += 1;
        if edge.kind.as_str() == "CALLS" {
            inbound_calls[target] += 1;
        }
        layout_edges.push(json!({
            "source": source,
            "target": target,
            "type": edge.kind.as_str(),
        }));
    }
    (degree, inbound_calls, layout_edges)
}

fn linked_projects_value(
    store: &QueryStore,
    source_project: &ProjectId,
    max_nodes: usize,
) -> Result<Vec<Value>, ApiError> {
    let mut source_nodes = store.list_nodes(source_project).map_err(internal)?;
    source_nodes.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    source_nodes.truncate(max_nodes);
    let source_ids = source_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str().to_owned(), index))
        .collect::<BTreeMap<_, _>>();
    let source_qn = source_nodes
        .iter()
        .map(|node| {
            (
                node.id.as_str().to_owned(),
                node.qualified_name.as_str().to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let source_edges = store.list_edges(source_project).map_err(internal)?;
    let grouped = grouped_cross_edges(&source_edges, source_project);
    linked_project_values(store, &source_ids, &source_qn, grouped, max_nodes)
}

fn grouped_cross_edges<'a>(
    edges: &'a [GraphEdge],
    source_project: &ProjectId,
) -> BTreeMap<String, Vec<&'a GraphEdge>> {
    let mut grouped = BTreeMap::<String, Vec<&GraphEdge>>::new();
    for edge in edges {
        if !edge.kind.as_str().starts_with("CROSS_") {
            continue;
        }
        let Some(target_project) = edge
            .properties
            .get("target_project")
            .and_then(Value::as_str)
            .filter(|project| *project != source_project.as_str())
        else {
            continue;
        };
        grouped
            .entry(target_project.to_owned())
            .or_default()
            .push(edge);
    }
    grouped
}

fn linked_project_values(
    store: &QueryStore,
    source_ids: &BTreeMap<String, usize>,
    source_qn: &BTreeMap<String, String>,
    grouped: BTreeMap<String, Vec<&GraphEdge>>,
    max_nodes: usize,
) -> Result<Vec<Value>, ApiError> {
    let count = grouped.len().min(16);
    let mut linked = Vec::with_capacity(count);
    for (position, (target_name, cross)) in grouped.into_iter().take(16).enumerate() {
        if let Some(value) = linked_project_value(
            store,
            source_ids,
            source_qn,
            &target_name,
            cross,
            max_nodes,
            position,
            count,
        )? {
            linked.push(value);
        }
    }
    Ok(linked)
}

#[allow(clippy::too_many_arguments)]
fn linked_project_value(
    store: &QueryStore,
    source_ids: &BTreeMap<String, usize>,
    source_qn: &BTreeMap<String, String>,
    target_name: &str,
    cross: Vec<&GraphEdge>,
    max_nodes: usize,
    position: usize,
    count: usize,
) -> Result<Option<Value>, ApiError> {
    let Some((target_qn, target_layout)) = target_layout(store, target_name, max_nodes)? else {
        return Ok(None);
    };
    let object = target_layout
        .as_object()
        .expect("layout values are always JSON objects");
    let cross_edges = cross_edges_value(cross, source_ids, source_qn, &target_qn);
    let angle = if count == 0 {
        0.0
    } else {
        2.0 * std::f64::consts::PI * usize_to_f64(position) / usize_to_f64(count)
    };
    Ok(Some(json!({
        "project": target_name,
        "nodes": object.get("nodes").cloned().unwrap_or_else(|| json!([])),
        "edges": object.get("edges").cloned().unwrap_or_else(|| json!([])),
        "offset": {
            "x": angle.cos() * 1_000.0,
            "y": angle.sin() * 1_000.0,
            "z": 0.0,
        },
        "cross_edges": cross_edges,
    })))
}

fn target_layout(
    store: &QueryStore,
    target_name: &str,
    max_nodes: usize,
) -> Result<Option<TargetLayout>, ApiError> {
    let Ok(target_project) = ProjectId::new(target_name) else {
        return Ok(None);
    };
    if store
        .get_project(&target_project)
        .map_err(internal)?
        .is_none()
    {
        return Ok(None);
    }
    let mut target_nodes = store.list_nodes(&target_project).map_err(internal)?;
    target_nodes.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    target_nodes.truncate(max_nodes);
    let target_qn = target_nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.qualified_name.as_str().to_owned(), index))
        .collect::<BTreeMap<_, _>>();
    let target_layout = layout_value(store, &target_project, max_nodes)?;
    Ok(Some((target_qn, target_layout)))
}

fn cross_edges_value(
    cross: Vec<&GraphEdge>,
    source_ids: &BTreeMap<String, usize>,
    source_qn: &BTreeMap<String, String>,
    target_qn: &BTreeMap<String, usize>,
) -> Vec<Value> {
    cross
        .into_iter()
        .filter_map(|edge| {
            let source = source_ids.get(edge.source.as_str())?;
            let qualified_name = source_qn.get(edge.target.as_str())?;
            let target = target_qn.get(qualified_name)?;
            Some(json!({
                "source": source,
                "target": target,
                "type": edge.kind.as_str(),
            }))
        })
        .collect::<Vec<_>>()
}

fn coordinates(index: usize, count: f64) -> (f64, f64, f64) {
    let position = usize_to_f64(index) + 0.5;
    let y = 1.0 - (2.0 * position / count);
    let radius = (1.0 - y * y).max(0.0).sqrt();
    let angle = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt()) * position;
    let scale = 24.0 * count.cbrt().max(1.0);
    (
        angle.cos() * radius * scale,
        y * scale,
        angle.sin() * radius * scale,
    )
}

#[allow(clippy::too_many_arguments)]
fn node_value(
    node: &GraphNode,
    index: usize,
    x: f64,
    y: f64,
    z: f64,
    degree: u64,
    inbound_calls: u64,
) -> Value {
    let mut object = Map::new();
    object.insert("id".to_owned(), json!(index));
    object.insert("x".to_owned(), json!(x));
    object.insert("y".to_owned(), json!(y));
    object.insert("z".to_owned(), json!(z));
    object.insert("label".to_owned(), json!(node.label.as_str()));
    object.insert("name".to_owned(), json!(node.name));
    object.insert(
        "qualified_name".to_owned(),
        json!(node.qualified_name.as_str()),
    );
    let degree = f64::from(u32::try_from(degree).unwrap_or(u32::MAX));
    object.insert("size".to_owned(), json!(1.0 + (degree + 1.0).ln()));
    object.insert("color".to_owned(), json!(label_color(node.label.as_str())));
    object.insert("in_calls".to_owned(), json!(inbound_calls));
    insert_optional_node_fields(&mut object, node);
    Value::Object(object)
}

fn insert_optional_node_fields(object: &mut Map<String, Value>, node: &GraphNode) {
    if let Some(path) = &node.file_path {
        object.insert("file_path".to_owned(), json!(path.as_str()));
    }
    if let Some(span) = node.source_span {
        object.insert("start_line".to_owned(), json!(span.start.row + 1));
        object.insert("end_line".to_owned(), json!(span.end.row + 1));
    }
    if let Some(status) = node.properties.get("status").and_then(Value::as_str) {
        object.insert("status".to_owned(), json!(status));
    }
}

fn label_color(label: &str) -> &'static str {
    const COLORS: &[&str] = &[
        "#7dd3fc", "#a78bfa", "#f472b6", "#fb7185", "#fbbf24", "#4ade80", "#2dd4bf", "#60a5fa",
        "#c084fc", "#f97316", "#94a3b8", "#e879f9",
    ];
    let hash = label.bytes().fold(2_166_136_261_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    COLORS[hash as usize % COLORS.len()]
}

fn usize_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).unwrap_or(u32::MAX))
}
