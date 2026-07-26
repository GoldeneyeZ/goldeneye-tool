use goldeneye_domain::{
    ByteSpan, EdgeKind, Generation, GraphEdge, GraphNode, GraphProperties, LanguageId, NodeId,
    NodeLabel, ProjectId, ProjectRelativePath, QualifiedName, SourcePoint, SourceSpan,
};
use serde_json::json;
use tree_sitter::Node;

use crate::error::ExtractionError as IndexError;

pub(super) fn path_stem(path: &ProjectRelativePath) -> String {
    let mut segments = path.as_str().split('/').collect::<Vec<_>>();
    if let Some(last) = segments.last_mut()
        && let Some((stem, _)) = last.rsplit_once('.')
    {
        *last = stem;
    }
    segments
        .into_iter()
        .map(qualified_segment)
        .collect::<Vec<_>>()
        .join(".")
}

pub(super) fn file_qualified_name(project: &ProjectId, path: &ProjectRelativePath) -> String {
    format!(
        "__file__.project.{}.path.{}",
        hex_encode_utf8(project.as_str()),
        hex_encode_utf8(path.as_str())
    )
}

fn hex_encode_utf8(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub(super) fn module_name(path: &ProjectRelativePath, language: &LanguageId) -> String {
    if language.as_str() != "go" {
        return path_stem(path);
    }
    path.as_str()
        .rsplit_once('/')
        .map_or_else(String::new, |(directory, _)| {
            directory
                .split('/')
                .map(qualified_segment)
                .collect::<Vec<_>>()
                .join(".")
        })
}

pub(super) fn qualified_segment(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() || character == '_' {
            if separator && !result.is_empty() {
                result.push('_');
            }
            separator = false;
            result.push(character);
        } else {
            separator = true;
        }
    }
    if result.is_empty() {
        "anonymous".to_owned()
    } else {
        result
    }
}

pub(super) fn stable_node_id(label: &str, qualified_name: &str) -> Result<NodeId, IndexError> {
    let hash = blake3::hash(format!("goldeneye-node-v1\0{label}\0{qualified_name}").as_bytes());
    Ok(NodeId::new(format!(
        "{}:{}",
        label.to_ascii_lowercase(),
        &hash.to_hex()[..32]
    ))?)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn graph_node(
    project: &ProjectId,
    path: &ProjectRelativePath,
    language: &LanguageId,
    label: &str,
    name: &str,
    qualified_name: &str,
    syntax_kind: &str,
    span: SourceSpan,
) -> Result<GraphNode, IndexError> {
    let mut properties = GraphProperties::new();
    properties.insert("language".into(), json!(language.as_str()));
    properties.insert("syntax_kind".into(), json!(syntax_kind));
    properties.insert("file_path".into(), json!(path.as_str()));
    Ok(GraphNode::new(
        project.clone(),
        stable_node_id(label, qualified_name)?,
        NodeLabel::new(label)?,
        name,
        QualifiedName::new(qualified_name)?,
        Some(path.clone()),
        Some(span),
        Generation::new(0),
    )?
    .with_properties(properties))
}

pub(super) fn project_node_id(project: &ProjectId) -> Result<NodeId, IndexError> {
    stable_node_id("Project", project.as_str())
}

pub(super) fn graph_edge(
    project: &ProjectId,
    source: NodeId,
    target: NodeId,
    kind: &str,
    discriminator: Option<String>,
    properties: GraphProperties,
) -> Result<GraphEdge, IndexError> {
    let edge = GraphEdge::new(
        project.clone(),
        source,
        target,
        EdgeKind::new(kind)?,
        Generation::new(0),
    )
    .with_properties(properties);
    match discriminator {
        Some(value) => edge.with_discriminator(value).map_err(IndexError::from),
        None => Ok(edge),
    }
}

pub(super) fn source_span(node: Node<'_>) -> Result<SourceSpan, IndexError> {
    let range = node.range();
    let start_byte = u64::try_from(range.start_byte)
        .map_err(|_| IndexError::CoordinateOverflow("start byte"))?;
    let end_byte =
        u64::try_from(range.end_byte).map_err(|_| IndexError::CoordinateOverflow("end byte"))?;
    let start_row =
        u64::try_from(range.start_point.row).map_err(|_| IndexError::CoordinateOverflow("row"))?;
    let start_column = u64::try_from(range.start_point.column)
        .map_err(|_| IndexError::CoordinateOverflow("column"))?;
    let end_row =
        u64::try_from(range.end_point.row).map_err(|_| IndexError::CoordinateOverflow("row"))?;
    let end_column = u64::try_from(range.end_point.column)
        .map_err(|_| IndexError::CoordinateOverflow("column"))?;
    Ok(SourceSpan::new(
        ByteSpan::new(start_byte, end_byte)?,
        SourcePoint::new(start_row, start_column),
        SourcePoint::new(end_row, end_column),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_and_module_names_preserve_current_normalization() {
        let cases = [
            ("src/my-file.test.rs", "src.my_file_test"),
            (".hidden", "anonymous"),
            ("odd---name/file.ts", "odd_name.file"),
        ];
        for (raw, expected) in cases {
            let path = ProjectRelativePath::new(raw).expect("valid test path");
            assert_eq!(path_stem(&path), expected, "{raw}");
        }

        let go = LanguageId::new("go").expect("valid language");
        let rust = LanguageId::new("rust").expect("valid language");
        let nested = ProjectRelativePath::new("pkg/http/server.go").expect("valid path");
        let root = ProjectRelativePath::new("main.go").expect("valid path");
        assert_eq!(module_name(&nested, &go), "pkg.http");
        assert_eq!(module_name(&root, &go), "");
        assert_eq!(module_name(&nested, &rust), "pkg.http.server");
    }

    #[test]
    fn file_qualified_names_encode_exact_project_and_path_bytes() {
        let project = ProjectId::new("repo").expect("valid project");
        let javascript = ProjectRelativePath::new("src/shared.js").expect("valid JavaScript path");
        let typescript = ProjectRelativePath::new("src/shared.ts").expect("valid TypeScript path");
        let hyphenated = ProjectRelativePath::new("src/a-b.rs").expect("valid hyphenated path");
        let underscored = ProjectRelativePath::new("src/a_b.rs").expect("valid underscored path");
        let properties = ProjectRelativePath::new(
            "spring-beans/src/test/resources/org/springframework/beans/factory/annotation/AutowiredConfigurationTests-custom.properties",
        )
        .expect("valid properties path");
        let xml = ProjectRelativePath::new(
            "spring-beans/src/test/resources/org/springframework/beans/factory/annotation/AutowiredConfigurationTests-custom.xml",
        )
        .expect("valid XML path");

        let javascript_name = file_qualified_name(&project, &javascript);
        let typescript_name = file_qualified_name(&project, &typescript);
        let hyphenated_name = file_qualified_name(&project, &hyphenated);
        let underscored_name = file_qualified_name(&project, &underscored);
        let properties_name = file_qualified_name(&project, &properties);
        let xml_name = file_qualified_name(&project, &xml);

        assert_eq!(
            javascript_name,
            "__file__.project.7265706f.path.7372632f7368617265642e6a73"
        );
        assert_eq!(
            typescript_name,
            "__file__.project.7265706f.path.7372632f7368617265642e7473"
        );
        assert_ne!(javascript_name, typescript_name);
        assert_ne!(hyphenated_name, underscored_name);
        assert_ne!(properties_name, xml_name);
        assert_ne!(
            stable_node_id("File", &javascript_name).expect("JavaScript file ID"),
            stable_node_id("File", &typescript_name).expect("TypeScript file ID")
        );
        assert_ne!(
            stable_node_id("File", &properties_name).expect("properties file ID"),
            stable_node_id("File", &xml_name).expect("XML file ID")
        );
    }

    #[test]
    fn file_qualified_names_frame_project_and_path_boundaries() {
        let dotted_project = ProjectId::new("repo.src").expect("valid dotted project");
        let plain_project = ProjectId::new("repo").expect("valid project");
        let plain_path = ProjectRelativePath::new("shared.js").expect("valid path");
        let dotted_path = ProjectRelativePath::new("src.shared.js").expect("valid dotted path");

        let dotted_project_name = file_qualified_name(&dotted_project, &plain_path);
        let dotted_path_name = file_qualified_name(&plain_project, &dotted_path);

        assert_eq!(
            dotted_project_name,
            "__file__.project.7265706f2e737263.path.7368617265642e6a73"
        );
        assert_eq!(
            dotted_path_name,
            "__file__.project.7265706f.path.7372632e7368617265642e6a73"
        );
        assert_ne!(dotted_project_name, dotted_path_name);
        assert_ne!(
            stable_node_id("File", &dotted_project_name).expect("dotted project file ID"),
            stable_node_id("File", &dotted_path_name).expect("dotted path file ID")
        );
    }

    #[test]
    fn file_qualified_names_hex_encode_unicode_utf8_bytes() {
        let project = ProjectId::new("répo").expect("valid Unicode project");
        let path = ProjectRelativePath::new("src/café.rs").expect("valid Unicode path");

        assert_eq!(
            file_qualified_name(&project, &path),
            "__file__.project.72c3a9706f.path.7372632f636166c3a92e7273"
        );
    }

    #[test]
    fn qualified_segments_and_ids_are_stable_and_domain_separated() {
        let cases = [
            ("alpha---beta", "alpha_beta"),
            (" already_ok ", "already_ok"),
            ("---", "anonymous"),
            ("naïve/type", "naïve_type"),
        ];
        for (raw, expected) in cases {
            assert_eq!(qualified_segment(raw), expected, "{raw}");
        }

        let first = stable_node_id("Class", "pkg.Widget").expect("stable id");
        let repeated = stable_node_id("Class", "pkg.Widget").expect("stable id");
        let other_label = stable_node_id("Function", "pkg.Widget").expect("stable id");
        let other_name = stable_node_id("Class", "pkg.Other").expect("stable id");
        assert_eq!(first, repeated);
        assert_ne!(first, other_label);
        assert_ne!(first, other_name);
        assert!(first.as_str().starts_with("class:"));
        assert_eq!(first.as_str().len(), "class:".len() + 32);
    }
}
