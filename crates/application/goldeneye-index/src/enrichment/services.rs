use super::libraries::{
    ASYNC_LIBRARIES, GRAPHQL_LIBRARIES, GRPC_LIBRARIES, HTTP_LIBRARIES, TRPC_LIBRARIES,
};
use super::routes::{canonical_route_path, first_string_literal, http_method};
use super::{
    BTreeMap, ExtractedCall, GraphEdge, GraphNode, GraphProperties, IndexError,
    MAX_SYNTHETIC_EDGES, ProjectId, ProjectRelativePath, Value, ensure_node, json, json_properties,
    push_edge,
};

pub(super) fn create_service_edges(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    calls: &[ExtractedCall],
    imports: &BTreeMap<(ProjectRelativePath, String), String>,
) -> Result<(), IndexError> {
    for call in calls.iter().take(MAX_SYNTHETIC_EDGES) {
        let context = imported_context(call, imports);
        if create_typed_service_edge(project, nodes, edges, call, &context)? {
            continue;
        }
        let Some(argument) = first_string_literal(&call.text) else {
            continue;
        };
        let local_call = has_local_call(edges, call);
        let is_global_fetch = call.short_name.eq_ignore_ascii_case("fetch") && !local_call;
        let is_http = is_global_fetch || HTTP_LIBRARIES.iter().any(|item| context.contains(item));
        let broker = ASYNC_LIBRARIES
            .iter()
            .find_map(|(pattern, broker)| context.contains(pattern).then_some(*broker));
        if is_http && (argument.starts_with('/') || argument.contains("://")) {
            let method = http_method(&call.callee_name).unwrap_or("ANY");
            let canonical = canonical_route_path(&argument);
            let qualified_name = format!("__route__{method}__{canonical}");
            let mut route_properties = GraphProperties::new();
            route_properties.insert("method".into(), json!(method));
            let route = ensure_node(
                project,
                nodes,
                "Route",
                &argument,
                &qualified_name,
                None,
                route_properties,
            )?;
            let properties = json_properties([
                ("callee", json!(call.callee_name)),
                ("url_path", json!(argument)),
                ("method", json!(method)),
            ]);
            push_edge(
                project,
                edges,
                &call.source,
                &route,
                "HTTP_CALLS",
                properties,
            )?;
        } else if let Some(broker) = broker.filter(|_| argument.len() > 2) {
            let qualified_name = format!("__route__{broker}__{argument}");
            let mut route_properties = GraphProperties::new();
            route_properties.insert("broker".into(), json!(broker));
            let route = ensure_node(
                project,
                nodes,
                "Route",
                &argument,
                &qualified_name,
                None,
                route_properties,
            )?;
            let properties = json_properties([
                ("callee", json!(call.callee_name)),
                ("url_path", json!(argument)),
                ("broker", json!(broker)),
            ]);
            push_edge(
                project,
                edges,
                &call.source,
                &route,
                "ASYNC_CALLS",
                properties,
            )?;
        }
    }
    Ok(())
}

fn create_typed_service_edge(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    call: &ExtractedCall,
    context: &str,
) -> Result<bool, IndexError> {
    let grpc_context = GRPC_LIBRARIES
        .iter()
        .any(|pattern| context.contains(pattern))
        || context.contains("ServiceClient")
        || context.contains("BlockingStub")
        || context.contains("Servicer");
    if grpc_context
        && let Some((service, method)) =
            grpc_service_method(context).or_else(|| grpc_service_method(&call.callee_name))
    {
        let route_name = format!("{service}/{method}");
        let route_qn = format!("__grpc__{route_name}");
        let route = ensure_node(
            project,
            nodes,
            "Route",
            &route_name,
            &route_qn,
            None,
            json_properties([("source", json!("grpc"))]),
        )?;
        push_edge(
            project,
            edges,
            &call.source,
            &route,
            "GRPC_CALLS",
            json_properties([
                ("callee", json!(call.callee_name)),
                ("service", json!(service)),
                ("method", json!(method)),
            ]),
        )?;
        return Ok(true);
    }

    let graphql_context = GRAPHQL_LIBRARIES.iter().any(|pattern| {
        context
            .to_ascii_lowercase()
            .contains(&pattern.to_ascii_lowercase())
    });
    if graphql_context {
        let operation = first_string_literal(&call.text)
            .as_deref()
            .and_then(graphql_operation)
            .unwrap_or(call.short_name.as_str())
            .to_owned();
        if !operation.is_empty() {
            let route_qn = format!("__graphql__{operation}");
            let route = ensure_node(
                project,
                nodes,
                "Route",
                &operation,
                &route_qn,
                None,
                json_properties([("source", json!("graphql"))]),
            )?;
            push_edge(
                project,
                edges,
                &call.source,
                &route,
                "GRAPHQL_CALLS",
                json_properties([
                    ("callee", json!(call.callee_name)),
                    ("operation", json!(operation)),
                ]),
            )?;
            return Ok(true);
        }
    }

    let trpc_context = TRPC_LIBRARIES
        .iter()
        .any(|pattern| context.contains(pattern));
    if trpc_context && let Some(procedure) = trpc_procedure(&call.callee_name) {
        let route_qn = format!("__trpc__{procedure}");
        let route = ensure_node(
            project,
            nodes,
            "Route",
            &procedure,
            &route_qn,
            None,
            json_properties([("source", json!("trpc"))]),
        )?;
        push_edge(
            project,
            edges,
            &call.source,
            &route,
            "TRPC_CALLS",
            json_properties([
                ("callee", json!(call.callee_name)),
                ("procedure", json!(procedure)),
            ]),
        )?;
        return Ok(true);
    }

    Ok(false)
}

fn grpc_service_method(value: &str) -> Option<(String, String)> {
    const SUFFIXES: &[&str] = &[
        "BlockingStub",
        "FutureStub",
        "AsyncStub",
        "AsyncClient",
        "Servicer",
        "Client",
        "Stub",
        "Grpc",
    ];
    value
        .split(|character: char| {
            character.is_whitespace()
                || matches!(
                    character,
                    '(' | ')' | '[' | ']' | '{' | '}' | ',' | '"' | '\''
                )
        })
        .filter(|candidate| candidate.contains('.'))
        .find_map(|candidate| {
            let candidate = candidate.trim_matches(|character: char| {
                !character.is_alphanumeric() && !matches!(character, '_' | '.')
            });
            let (service_path, method) = candidate.rsplit_once('.')?;
            if !identifier_fragment(method) {
                return None;
            }
            let raw_service = service_path.rsplit('.').next()?;
            let raw_service = raw_service.strip_prefix("pb.New").unwrap_or(raw_service);
            let raw_service = raw_service.strip_prefix("New").unwrap_or(raw_service);
            let service = SUFFIXES
                .iter()
                .find_map(|suffix| raw_service.strip_suffix(suffix))
                .filter(|service| !service.is_empty())?;
            Some((service.to_owned(), method.to_owned()))
        })
}

fn graphql_operation(value: &str) -> Option<&str> {
    let value = value.trim_start();
    let value = ["query", "mutation", "subscription"]
        .into_iter()
        .find_map(|prefix| value.strip_prefix(prefix).map(str::trim_start))
        .unwrap_or(value);
    let operation = value
        .split(|character: char| character.is_whitespace() || matches!(character, '(' | '{'))
        .next()
        .unwrap_or_default();
    identifier_fragment(operation).then_some(operation)
}

fn trpc_procedure(value: &str) -> Option<String> {
    let mut procedure = value.trim().to_owned();
    for suffix in [
        ".useMutation",
        ".useQuery",
        ".subscribe",
        ".mutation",
        ".mutate",
        ".query",
    ] {
        if procedure.ends_with(suffix) {
            procedure.truncate(procedure.len() - suffix.len());
            break;
        }
    }
    if let Some(index) = procedure.find("trpc.") {
        procedure.drain(..index + "trpc.".len());
    }
    (!procedure.is_empty() && procedure.split('.').all(identifier_fragment)).then_some(procedure)
}

pub(super) fn identifier_fragment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
}

fn imported_context(
    call: &ExtractedCall,
    imports: &BTreeMap<(ProjectRelativePath, String), String>,
) -> String {
    let alias = call
        .callee_name
        .split(['.', ':', '/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(&call.short_name);
    imports
        .get(&(call.file.clone(), alias.to_owned()))
        .map_or_else(
            || call.callee_name.clone(),
            |module| format!("{module}.{}", call.callee_name),
        )
}

fn has_local_call(edges: &[GraphEdge], call: &ExtractedCall) -> bool {
    edges.iter().any(|edge| {
        edge.source == call.source
            && edge.kind.as_str() == "CALLS"
            && edge
                .properties
                .get("callee")
                .and_then(Value::as_str)
                .is_none_or(|callee| callee == call.callee_name || callee == call.short_name)
    })
}
