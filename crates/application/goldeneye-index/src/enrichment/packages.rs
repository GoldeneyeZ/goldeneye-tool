use super::{
    ExtractedImport, GraphEdge, GraphNode, IndexError, MAX_SYNTHETIC_EDGES, NodeId, ProjectId,
    ProjectRelativePath, SourceFile, Value, json, json_properties, push_edge,
};

pub(super) fn create_package_links(
    project: &ProjectId,
    nodes: &[GraphNode],
    edges: &mut Vec<GraphEdge>,
    imports: &[ExtractedImport],
    sources: &[SourceFile],
) -> Result<(), IndexError> {
    let manifests = sources
        .iter()
        .filter_map(parse_manifest)
        .collect::<Vec<_>>();
    if manifests.is_empty() {
        return Ok(());
    }
    for import in imports.iter().take(MAX_SYNTHETIC_EDGES) {
        let Some(manifest) = manifests.iter().find(|manifest| {
            import.module_path == manifest.name
                || import
                    .module_path
                    .strip_prefix(&manifest.name)
                    .is_some_and(|suffix| suffix.starts_with(['/', '.', ':']))
        }) else {
            continue;
        };
        let source = file_node(nodes, &import.file);
        let target = manifest_target(nodes, manifest, &import.module_path);
        if let (Some(source), Some(target)) = (source, target) {
            push_edge(
                project,
                edges,
                source,
                target,
                "IMPORTS",
                json_properties([
                    ("strategy", json!("package_manifest")),
                    ("confidence", json!(0.95)),
                    ("package", json!(manifest.name)),
                ]),
            )?;
        }
    }
    Ok(())
}

#[derive(Debug)]
struct ManifestEntry {
    name: String,
    root: String,
    entry: Option<String>,
}

fn parse_manifest(source: &SourceFile) -> Option<ManifestEntry> {
    let path = source.path.as_str();
    let root = path
        .rsplit_once('/')
        .map_or("", |(root, _)| root)
        .to_owned();
    let text = String::from_utf8_lossy(&source.source);
    if path.ends_with("package.json") {
        let value: Value = serde_json::from_slice(&source.source).ok()?;
        let name = value.get("name")?.as_str()?.to_owned();
        let entry = value
            .get("module")
            .or_else(|| value.get("main"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        return Some(ManifestEntry { name, root, entry });
    }
    if path.ends_with("go.mod") {
        let name = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("module "))?
            .trim()
            .to_owned();
        return Some(ManifestEntry {
            name,
            root,
            entry: None,
        });
    }
    if path.ends_with("Cargo.toml") || path.ends_with("pyproject.toml") {
        let mut in_package = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_package = matches!(trimmed, "[package]" | "[project]" | "[tool.poetry]");
            } else if in_package
                && let Some(value) = trimmed
                    .strip_prefix("name")
                    .and_then(|rest| rest.trim_start().strip_prefix('='))
            {
                let name = value.trim().trim_matches(['"', '\'']).to_owned();
                if !name.is_empty() {
                    return Some(ManifestEntry {
                        name,
                        root,
                        entry: None,
                    });
                }
            }
        }
    }
    None
}

fn file_node<'a>(nodes: &'a [GraphNode], path: &ProjectRelativePath) -> Option<&'a NodeId> {
    nodes
        .iter()
        .find(|node| node.label.as_str() == "File" && node.file_path.as_ref() == Some(path))
        .map(|node| &node.id)
}

fn manifest_target<'a>(
    nodes: &'a [GraphNode],
    manifest: &ManifestEntry,
    module_path: &str,
) -> Option<&'a NodeId> {
    let suffix = module_path
        .strip_prefix(&manifest.name)
        .unwrap_or_default()
        .trim_start_matches(['/', '.', ':']);
    let entry = manifest.entry.as_deref().unwrap_or(suffix);
    let joined = [manifest.root.as_str(), entry.trim_start_matches("./")]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    nodes
        .iter()
        .filter(|node| node.label.as_str() == "File")
        .filter_map(|node| Some((node.file_path.as_ref()?.as_str(), &node.id)))
        .find(|(path, _)| {
            *path == joined
                || path.strip_suffix(".rs") == Some(joined.as_str())
                || path.strip_suffix(".go") == Some(joined.as_str())
                || path.strip_suffix(".py") == Some(joined.as_str())
                || path.strip_suffix(".js") == Some(joined.as_str())
                || path.strip_suffix(".ts") == Some(joined.as_str())
                || (!joined.is_empty() && path.starts_with(&format!("{joined}/")))
                || (entry.is_empty() && path.starts_with(&manifest.root))
        })
        .map(|(_, id)| id)
}
