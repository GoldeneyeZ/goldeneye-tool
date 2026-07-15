use goldeneye_domain::{
    EdgeKind, Generation, GraphEdge, GraphNode, GraphProperties, NodeId, NodeLabel, ProjectId,
    ProjectRecord, QualifiedName,
};
use serde_json::json;

use crate::IndexError;

pub(crate) fn project_node(project: &ProjectRecord) -> Result<GraphNode, IndexError> {
    let qualified_name = project.id.as_str();
    let mut properties = GraphProperties::new();
    properties.insert("root_path".into(), json!(project.root_path));
    Ok(GraphNode::new(
        project.id.clone(),
        stable_node_id("Project", qualified_name)?,
        NodeLabel::new("Project")?,
        qualified_name,
        QualifiedName::new(qualified_name)?,
        None,
        None,
        Generation::new(0),
    )?
    .with_properties(properties))
}

pub(crate) fn branch_node(project: &ProjectRecord) -> Result<GraphNode, IndexError> {
    let qualified_name = format!("{}.__branch__.working-tree", project.id.as_str());
    let mut properties = GraphProperties::new();
    properties.insert("branch".into(), json!("working-tree"));
    Ok(GraphNode::new(
        project.id.clone(),
        stable_node_id("Branch", &qualified_name)?,
        NodeLabel::new("Branch")?,
        "working-tree",
        QualifiedName::new(qualified_name)?,
        None,
        None,
        Generation::new(0),
    )?
    .with_properties(properties))
}

pub(crate) fn project_has_branch(
    project: &ProjectId,
    branch: &GraphNode,
) -> Result<GraphEdge, IndexError> {
    graph_edge(
        project,
        stable_node_id("Project", project.as_str())?,
        branch.id.clone(),
        "HAS_BRANCH",
    )
}

pub(crate) fn project_contains_file(
    project: &ProjectId,
    file_node: &GraphNode,
) -> Result<GraphEdge, IndexError> {
    let branch_qualified_name = format!("{}.__branch__.working-tree", project.as_str());
    graph_edge(
        project,
        stable_node_id("Branch", &branch_qualified_name)?,
        file_node.id.clone(),
        "CONTAINS_FILE",
    )
}

fn stable_node_id(label: &str, qualified_name: &str) -> Result<NodeId, IndexError> {
    let hash = blake3::hash(format!("goldeneye-node-v1\0{label}\0{qualified_name}").as_bytes());
    Ok(NodeId::new(format!(
        "{}:{}",
        label.to_ascii_lowercase(),
        &hash.to_hex()[..32]
    ))?)
}

fn graph_edge(
    project: &ProjectId,
    source: NodeId,
    target: NodeId,
    kind: &str,
) -> Result<GraphEdge, IndexError> {
    Ok(GraphEdge::new(
        project.clone(),
        source,
        target,
        EdgeKind::new(kind)?,
        Generation::new(0),
    ))
}
