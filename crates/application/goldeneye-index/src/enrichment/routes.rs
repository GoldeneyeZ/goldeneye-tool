use super::{
    GraphEdge, GraphNode, IndexError, MAX_LITERAL_BYTES, ProjectId, SourceFile, ensure_node, json,
    json_properties, push_edge,
};

pub(super) fn create_decorator_routes(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    sources: &[SourceFile],
) -> Result<(), IndexError> {
    let mut declarations = Vec::new();
    for source in sources {
        let text = String::from_utf8_lossy(&source.source);
        let mut byte_offset = 0_u64;
        for line in text.lines() {
            if let (Some(method), Some(path)) = (
                route_method_from_annotation(line),
                first_string_literal(line),
            ) && path.starts_with('/')
            {
                declarations.push((source.path.clone(), byte_offset, method, path));
            }
            byte_offset = byte_offset.saturating_add(line.len() as u64 + 1);
        }
    }

    for (path, annotation_byte, method, route_path) in declarations {
        let handler = nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                node.file_path.as_ref() == Some(&path)
                    && matches!(node.label.as_str(), "Function" | "Method")
                    && node
                        .source_span
                        .as_ref()
                        .is_some_and(|span| span.bytes.start >= annotation_byte)
            })
            .min_by_key(|(_, node)| {
                node.source_span
                    .as_ref()
                    .map_or(u64::MAX, |span| span.bytes.start)
            })
            .map(|(index, node)| {
                (
                    index,
                    node.id.clone(),
                    node.qualified_name.as_str().to_owned(),
                )
            });
        let Some((index, handler_id, handler_qn)) = handler else {
            continue;
        };
        nodes[index]
            .properties
            .insert("route_path".into(), json!(route_path));
        nodes[index]
            .properties
            .insert("route_method".into(), json!(method));
        let canonical = canonical_route_path(&route_path);
        let route_qn = format!("__route__{method}__{canonical}");
        let route = ensure_node(
            project,
            nodes,
            "Route",
            &route_path,
            &route_qn,
            Some(path),
            json_properties([("method", json!(method)), ("source", json!("decorator"))]),
        )?;
        push_edge(
            project,
            edges,
            &handler_id,
            &route,
            "HANDLES",
            json_properties([("handler", json!(handler_qn))]),
        )?;
    }
    Ok(())
}

pub(super) fn first_string_literal(text: &str) -> Option<String> {
    let mut quote = None;
    let mut escaped = false;
    let mut output = String::new();
    for character in text.chars() {
        if let Some(expected) = quote {
            if escaped {
                if output.len() < MAX_LITERAL_BYTES {
                    output.push(character);
                }
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == expected {
                return (!output.is_empty()).then_some(output);
            } else if output.len() < MAX_LITERAL_BYTES {
                output.push(character);
            }
        } else if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
        }
    }
    None
}

pub(super) fn http_method(callee: &str) -> Option<&'static str> {
    const METHODS: &[(&str, Option<&str>)] = &[
        (".get", Some("GET")),
        (".Get", Some("GET")),
        (".GET", Some("GET")),
        (".post", Some("POST")),
        (".Post", Some("POST")),
        (".POST", Some("POST")),
        (".put", Some("PUT")),
        (".Put", Some("PUT")),
        (".delete", Some("DELETE")),
        (".Delete", Some("DELETE")),
        (".patch", Some("PATCH")),
        (".Patch", Some("PATCH")),
        (".head", Some("HEAD")),
        (".options", Some("OPTIONS")),
        ("GetAsync", Some("GET")),
        ("PostAsync", Some("POST")),
        ("PutAsync", Some("PUT")),
        ("DeleteAsync", Some("DELETE")),
        ("getForObject", Some("GET")),
        ("getForEntity", Some("GET")),
        ("postForObject", Some("POST")),
        ("postForEntity", Some("POST")),
    ];
    METHODS
        .iter()
        .find_map(|(suffix, method)| callee.ends_with(suffix).then_some(*method).flatten())
}

pub(super) fn canonical_route_path(input: &str) -> String {
    let mut route = input.trim();
    if let Some(authority_and_path) = route
        .strip_prefix("https://")
        .or_else(|| route.strip_prefix("http://"))
    {
        route = authority_and_path
            .find('/')
            .map_or("/", |index| &authority_and_path[index..]);
    } else if let Some(authority_and_path) = route.strip_prefix("//") {
        route = authority_and_path
            .find('/')
            .map_or("/", |index| &authority_and_path[index..]);
    }
    if let Some(index) = route.find(['?', '#']) {
        route = &route[..index];
    }
    if route.is_empty() {
        route = "/";
    }

    let chars = route.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(route.len());
    let mut index = 0;
    while index < chars.len() {
        let at_segment_start = output.is_empty() || output.ends_with('/');
        let start = chars[index];
        let terminator = match start {
            '{' => Some('}'),
            '<' => Some('>'),
            '$' if chars.get(index + 1) == Some(&'{') => Some('}'),
            ':' if at_segment_start => Some('/'),
            _ => None,
        };
        if let Some(terminator) = terminator {
            index += usize::from(start == '$') + 1;
            while index < chars.len()
                && chars[index] != terminator
                && (terminator != '/' || chars[index] != '/')
            {
                index += 1;
            }
            if index < chars.len() && terminator != '/' {
                index += 1;
            }
            output.push_str("{}");
            continue;
        }
        output.push(start);
        index += 1;
    }
    output
}

pub(super) fn route_method_from_annotation(line: &str) -> Option<&'static str> {
    let lower = line.to_ascii_lowercase();
    let is_route = line.trim_start().starts_with(['@', '#', '['])
        || lower.contains("route(")
        || lower.contains("mapping(");
    if !is_route {
        return None;
    }
    [
        ("delete", "DELETE"),
        ("patch", "PATCH"),
        ("options", "OPTIONS"),
        ("head", "HEAD"),
        ("post", "POST"),
        ("put", "PUT"),
        ("get", "GET"),
    ]
    .into_iter()
    .find_map(|(needle, method)| lower.contains(needle).then_some(method))
    .or(Some("ANY"))
}
