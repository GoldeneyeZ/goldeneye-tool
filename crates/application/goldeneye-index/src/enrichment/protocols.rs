use super::routes::first_string_literal;
use super::services::identifier_fragment;
use super::{
    GraphEdge, GraphNode, GraphProperties, IndexError, NodeId, ProjectId, ProjectRelativePath,
    SourceFile, ensure_node, json, json_properties, push_edge,
};

#[allow(clippy::too_many_lines)]
pub(super) fn create_protocol_handlers(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    sources: &[SourceFile],
) -> Result<(), IndexError> {
    struct Declaration {
        path: ProjectRelativePath,
        byte_offset: u64,
        route_name: String,
        route_qn: String,
        source: &'static str,
        hint: Option<String>,
        broker: Option<&'static str>,
    }

    let mut declarations = Vec::new();
    for source in sources {
        let text = String::from_utf8_lossy(&source.source);
        let path = source.path.as_str().to_ascii_lowercase();
        let extension = path.rsplit_once('.').map_or("", |(_, extension)| extension);
        let is_proto = extension == "proto";
        let is_graphql_schema = matches!(extension, "graphql" | "gql");
        let mut proto_service = None::<String>;
        let mut graphql_type = None::<String>;
        let mut trpc_router = None::<String>;
        let mut byte_offset = 0_u64;

        for line in text.lines() {
            let trimmed = line.trim();
            let lower = trimmed.to_ascii_lowercase();
            if is_proto {
                if let Some(rest) = trimmed.strip_prefix("service ") {
                    proto_service = rest
                        .split(|character: char| character.is_whitespace() || character == '{')
                        .find(|value| identifier_fragment(value))
                        .map(ToOwned::to_owned);
                } else if let (Some(service), Some(rest)) =
                    (proto_service.as_ref(), trimmed.strip_prefix("rpc "))
                    && let Some(method) = rest
                        .split(|character: char| character.is_whitespace() || character == '(')
                        .find(|value| identifier_fragment(value))
                {
                    declarations.push(Declaration {
                        path: source.path.clone(),
                        byte_offset,
                        route_name: format!("{service}/{method}"),
                        route_qn: format!("__grpc__{service}/{method}"),
                        source: "grpc",
                        hint: Some(method.to_owned()),
                        broker: None,
                    });
                }
                if trimmed == "}" {
                    proto_service = None;
                }
            }

            if is_graphql_schema {
                if let Some(rest) = trimmed
                    .strip_prefix("type ")
                    .or_else(|| trimmed.strip_prefix("extend type "))
                {
                    graphql_type = rest
                        .split(|character: char| character.is_whitespace() || character == '{')
                        .next()
                        .filter(|kind| matches!(*kind, "Query" | "Mutation" | "Subscription"))
                        .map(ToOwned::to_owned);
                } else if graphql_type.is_some()
                    && !trimmed.starts_with(['#', '}', '@'])
                    && let Some(field) = trimmed
                        .split(|character: char| {
                            character.is_whitespace() || matches!(character, ':' | '(')
                        })
                        .find(|value| identifier_fragment(value))
                {
                    declarations.push(Declaration {
                        path: source.path.clone(),
                        byte_offset,
                        route_name: field.to_owned(),
                        route_qn: format!("__graphql__{field}"),
                        source: "graphql",
                        hint: Some(field.to_owned()),
                        broker: None,
                    });
                }
                if trimmed == "}" {
                    graphql_type = None;
                }
            }

            if lower.contains("createtrpcrouter")
                && let Some(left) = trimmed.split('=').next()
                && let Some(name) = left
                    .split(|character: char| !character.is_alphanumeric() && character != '_')
                    .rfind(|value| identifier_fragment(value))
            {
                let name = name.strip_suffix("Router").unwrap_or(name);
                trpc_router = Some(lowercase_first(name));
            } else if let Some(router) = trpc_router.as_ref()
                && (lower.contains(".query(")
                    || lower.contains(".mutation(")
                    || lower.contains(".subscription(")
                    || lower.contains("publicprocedure"))
                && let Some(procedure) = trimmed
                    .split_once(':')
                    .map(|(name, _)| name.trim())
                    .filter(|name| identifier_fragment(name))
            {
                let route_name = format!("{router}.{procedure}");
                declarations.push(Declaration {
                    path: source.path.clone(),
                    byte_offset,
                    route_qn: format!("__trpc__{route_name}"),
                    route_name,
                    source: "trpc",
                    hint: Some(procedure.to_owned()),
                    broker: None,
                });
            }
            if trimmed.starts_with("});") || trimmed == "}" {
                trpc_router = None;
            }

            if (lower.starts_with("@query")
                || lower.starts_with("@mutation")
                || lower.starts_with("@subscription"))
                && !is_graphql_schema
            {
                let explicit =
                    first_string_literal(trimmed).filter(|value| identifier_fragment(value));
                declarations.push(Declaration {
                    path: source.path.clone(),
                    byte_offset,
                    route_name: explicit.clone().unwrap_or_default(),
                    route_qn: explicit.as_ref().map_or_else(
                        || "__graphql__".to_owned(),
                        |name| format!("__graphql__{name}"),
                    ),
                    source: "graphql",
                    hint: explicit,
                    broker: None,
                });
            }

            if let (Some(broker), Some(topic)) =
                (listener_broker(&lower), first_string_literal(trimmed))
            {
                declarations.push(Declaration {
                    path: source.path.clone(),
                    byte_offset,
                    route_name: topic.clone(),
                    route_qn: format!("__route__{broker}__{topic}"),
                    source: "async_listener",
                    hint: None,
                    broker: Some(broker),
                });
            }
            byte_offset = byte_offset.saturating_add(line.len() as u64 + 1);
        }
    }

    for mut declaration in declarations {
        let Some((handler_id, handler_qn, handler_name)) = handler_for_declaration(
            nodes,
            &declaration.path,
            declaration.byte_offset,
            declaration.hint.as_deref(),
        ) else {
            continue;
        };
        if declaration.source == "graphql" && declaration.route_name.is_empty() {
            declaration.route_name.clone_from(&handler_name);
            declaration.route_qn = format!("__graphql__{handler_name}");
        }
        let mut properties = GraphProperties::new();
        properties.insert("source".to_owned(), json!(declaration.source));
        if let Some(broker) = declaration.broker {
            properties.insert("broker".to_owned(), json!(broker));
        }
        let route = ensure_node(
            project,
            nodes,
            "Route",
            &declaration.route_name,
            &declaration.route_qn,
            Some(declaration.path),
            properties,
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

fn handler_for_declaration(
    nodes: &[GraphNode],
    path: &ProjectRelativePath,
    byte_offset: u64,
    hint: Option<&str>,
) -> Option<(NodeId, String, String)> {
    let functions = nodes.iter().filter(|node| {
        node.file_path.as_ref() == Some(path)
            && matches!(node.label.as_str(), "Function" | "Method")
    });
    let handler = hint
        .and_then(|hint| {
            functions
                .clone()
                .filter(|node| node.name == hint || node.name.ends_with(hint))
                .min_by_key(|node| {
                    node.source_span
                        .as_ref()
                        .map_or(u64::MAX, |span| span.bytes.start.abs_diff(byte_offset))
                })
        })
        .or_else(|| {
            functions
                .filter(|node| {
                    node.source_span
                        .as_ref()
                        .is_some_and(|span| span.bytes.start >= byte_offset)
                })
                .min_by_key(|node| {
                    node.source_span
                        .as_ref()
                        .map_or(u64::MAX, |span| span.bytes.start)
                })
        })
        .or_else(|| {
            nodes
                .iter()
                .find(|node| node.file_path.as_ref() == Some(path) && node.label.as_str() == "File")
        })?;
    Some((
        handler.id.clone(),
        handler.qualified_name.as_str().to_owned(),
        handler.name.clone(),
    ))
}

fn listener_broker(lower_line: &str) -> Option<&'static str> {
    [
        ("kafkalistener", "kafka"),
        ("sqslistener", "sqs"),
        ("rabbitlistener", "rabbitmq"),
        ("eventpattern", "nestjs"),
        ("messagepattern", "nestjs"),
        ("pubsubsubscription", "pubsub"),
        ("celery.task", "celery"),
        ("sidekiq", "sidekiq"),
        ("queue.process", "bull"),
    ]
    .into_iter()
    .find_map(|(pattern, broker)| lower_line.contains(pattern).then_some(broker))
}

fn lowercase_first(value: &str) -> String {
    let mut characters = value.chars();
    let Some(first) = characters.next() else {
        return String::new();
    };
    first.to_lowercase().chain(characters).collect()
}
