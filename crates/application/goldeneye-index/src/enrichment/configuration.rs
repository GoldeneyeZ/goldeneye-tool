use super::{
    GraphEdge, GraphNode, IndexError, ProjectId, ProjectRelativePath, json, json_properties,
    push_edge,
};

pub(super) fn create_config_links(
    project: &ProjectId,
    nodes: &[GraphNode],
    edges: &mut Vec<GraphEdge>,
) -> Result<(), IndexError> {
    let config = nodes
        .iter()
        .filter(|node| matches!(node.label.as_str(), "Variable" | "Field"))
        .filter(|node| node.file_path.as_ref().is_some_and(is_config_path))
        .filter_map(|node| {
            let tokens = normalize_config_key(&node.name);
            (tokens.len() >= 2 && tokens.iter().all(|token| token.len() >= 3))
                .then(|| (node.id.clone(), node.name.clone(), tokens.join("_")))
        })
        .take(4_096)
        .collect::<Vec<_>>();
    if config.is_empty() {
        return Ok(());
    }
    let code = nodes
        .iter()
        .filter(|node| {
            matches!(
                node.label.as_str(),
                "Function" | "Variable" | "Class" | "Struct"
            )
        })
        .filter(|node| !node.file_path.as_ref().is_some_and(is_config_path))
        .filter_map(|node| {
            let normalized = normalize_config_key(&node.name).join("_");
            (!normalized.is_empty()).then(|| (node.id.clone(), normalized))
        })
        .take(8_192)
        .collect::<Vec<_>>();

    for (config_id, config_name, config_normalized) in config {
        for (code_id, code_normalized) in &code {
            let confidence = if *code_normalized == config_normalized {
                Some(0.85)
            } else if code_normalized.contains(&config_normalized) {
                Some(0.75)
            } else {
                None
            };
            let Some(confidence) = confidence else {
                continue;
            };
            push_edge(
                project,
                edges,
                code_id,
                &config_id,
                "CONFIGURES",
                json_properties([
                    ("strategy", json!("key_symbol")),
                    ("confidence", json!(confidence)),
                    ("config_key", json!(config_name)),
                ]),
            )?;
        }
    }
    Ok(())
}

pub(super) fn normalize_config_key(value: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let characters = value.chars().collect::<Vec<_>>();
    for (index, character) in characters.iter().copied().enumerate() {
        if matches!(character, '_' | '-' | '.' | ' ' | '/' | ':') {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            continue;
        }
        let camel_boundary = character.is_ascii_uppercase()
            && !current.is_empty()
            && characters
                .get(index.wrapping_sub(1))
                .is_some_and(char::is_ascii_lowercase);
        if camel_boundary {
            words.push(std::mem::take(&mut current));
        }
        current.push(character.to_ascii_lowercase());
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

fn is_config_path(path: &ProjectRelativePath) -> bool {
    let lower = path.as_str().to_ascii_lowercase();
    [
        ".json",
        ".yaml",
        ".yml",
        ".toml",
        ".ini",
        ".conf",
        ".config",
        ".env",
        ".properties",
        ".xml",
    ]
    .iter()
    .any(|extension| lower.ends_with(extension))
}
