use super::routes::first_string_literal;
use super::{
    ExtractedCall, GraphEdge, GraphNode, GraphProperties, IndexError, MAX_SYNTHETIC_EDGES,
    ProjectId, ensure_node, json, json_properties, push_edge,
};

pub(super) fn create_environment_edges(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    calls: &[ExtractedCall],
) -> Result<(), IndexError> {
    for call in calls.iter().take(MAX_SYNTHETIC_EDGES) {
        if !is_environment_access(&call.callee_name) {
            continue;
        }
        let Some(key) = first_string_literal(&call.text) else {
            continue;
        };
        if !is_env_name(&key) {
            continue;
        }
        let qualified_name = format!("__env__{key}");
        let mut properties = GraphProperties::new();
        properties.insert("env_key".into(), json!(key));
        let target = ensure_node(
            project,
            nodes,
            "EnvVar",
            &key,
            &qualified_name,
            None,
            properties,
        )?;
        push_edge(
            project,
            edges,
            &call.source,
            &target,
            "CONFIGURES",
            json_properties([("strategy", json!("env_access"))]),
        )?;
    }
    Ok(())
}

fn is_environment_access(callee: &str) -> bool {
    let lower = callee.to_ascii_lowercase();
    lower.contains("getenv")
        || lower.contains("get_environment_variable")
        || lower.contains("getenvironmentvariable")
        || lower.ends_with("env::var")
        || lower.ends_with("env.var")
}

fn is_env_name(value: &str) -> bool {
    value.len() >= 2
        && value.chars().all(|character| {
            character.is_ascii_uppercase() || character == '_' || character.is_ascii_digit()
        })
        && value
            .chars()
            .any(|character| character.is_ascii_uppercase())
}
