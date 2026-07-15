use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use goldeneye_domain::{
    EdgeKind, Generation, GraphEdge, GraphNode, GraphProperties, NodeId, NodeLabel, ProjectId,
    ProjectRelativePath, QualifiedName,
};
use goldeneye_ports::{
    IndexExtractedCall as ExtractedCall, IndexExtractedImport as ExtractedImport,
};
use serde_json::{Value, json};

use crate::IndexError;

const MAX_SYNTHETIC_NODES: usize = 8_192;
const MAX_SYNTHETIC_EDGES: usize = 32_768;
const MAX_LITERAL_BYTES: usize = 512;

const HTTP_LIBRARIES: &[&str] = &[
    "requests",
    "httpx",
    "aiohttp",
    "urllib",
    "urllib3",
    "httplib2",
    "pycurl",
    "treq",
    "uplink",
    "axios",
    "superagent",
    "needle",
    "node-fetch",
    "undici",
    "ofetch",
    "wretch",
    "sindresorhus/ky",
    "phin",
    "net/http",
    "resty",
    "sling",
    "heimdall",
    "gentleman",
    "retryablehttp",
    "HttpClient",
    "OkHttp",
    "okhttp3",
    "RestTemplate",
    "WebClient",
    "Unirest",
    "AsyncHttpClient",
    "apache.http",
    "Retrofit",
    "Feign",
    "ktor.client",
    "kittinunf.fuel",
    "reqwest",
    "hyper",
    "surf",
    "ureq",
    "isahc",
    "attohttpc",
    "RestSharp",
    "Flurl",
    "Refit",
    "HTTParty",
    "Faraday",
    "RestClient",
    "Typhoeus",
    "Excon",
    "Net::HTTP",
    "Guzzle",
    "guzzle",
    "curl",
    "Symfony\\HttpClient",
    "cpr",
    "cpp-httplib",
    "Poco.Net",
    "Beast",
    "Alamofire",
    "Moya",
    "URLSession",
    "Dio",
    "dio",
    "package:http",
    "Chopper",
    "HTTPoison",
    "Tesla",
    "Finch",
    "Mint.HTTP",
    "sttp",
    "akka.http",
    "http4s",
    "scalaj",
    "wreq",
    "http-client",
    "http-conduit",
    "servant-client",
    "Network.HTTP",
    "socket.http",
    "resty.http",
];

const GRPC_LIBRARIES: &[&str] = &[
    "google.golang.org/grpc",
    "grpc.",
    "grpcio",
    "io.grpc",
    "ManagedChannel",
    "Grpc.Net.Client",
    "GrpcChannel",
    "Grpc.Core",
    "@grpc/grpc-js",
    "grpc-web",
    "tonic",
    "package:grpc",
];

const GRAPHQL_LIBRARIES: &[&str] = &[
    "graphql",
    "apollo",
    "urql",
    "relay",
    "gqlgen",
    "juniper",
    "async-graphql",
    "graphene",
    "strawberry",
];

const TRPC_LIBRARIES: &[&str] = &[
    "@trpc/server",
    "@trpc/client",
    "@trpc/react-query",
    "createTRPCRouter",
    "trpc.",
];

const ASYNC_LIBRARIES: &[(&str, &str)] = &[
    ("cloudtasks", "cloud_tasks"),
    ("cloud_tasks", "cloud_tasks"),
    ("cloud.tasks", "cloud_tasks"),
    ("CloudTasks", "cloud_tasks"),
    ("pubsub", "pubsub"),
    ("cloud.pubsub", "pubsub"),
    ("PubSub", "pubsub"),
    ("aws-sdk-go/service/sqs", "sqs"),
    ("aws-sdk-go.service.sqs", "sqs"),
    ("aws_sdk_sqs", "sqs"),
    ("Amazon.SQS", "sqs"),
    ("@aws-sdk/client-sqs", "sqs"),
    ("boto3.client.sqs", "sqs"),
    ("aws-sdk-go/service/sns", "sns"),
    ("aws_sdk_sns", "sns"),
    ("Amazon.SNS", "sns"),
    ("@aws-sdk/client-sns", "sns"),
    ("eventbridge", "eventbridge"),
    ("EventBridge", "eventbridge"),
    ("aws_sdk_lambda", "lambda"),
    ("@aws-sdk/client-lambda", "lambda"),
    ("stepfunctions", "stepfunctions"),
    ("ServiceBus", "servicebus"),
    ("Azure.Messaging", "servicebus"),
    ("kafka", "kafka"),
    ("Kafka", "kafka"),
    ("kafkajs", "kafka"),
    ("sarama", "kafka"),
    ("rdkafka", "kafka"),
    ("confluent", "kafka"),
    ("amqp", "rabbitmq"),
    ("AMQP", "rabbitmq"),
    ("amqplib", "rabbitmq"),
    ("RabbitMQ", "rabbitmq"),
    ("lapin", "rabbitmq"),
    ("MassTransit", "rabbitmq"),
    ("nats", "nats"),
    ("NATS", "nats"),
    ("ioredis", "redis"),
    ("celery", "celery"),
    ("Celery", "celery"),
    ("dramatiq", "dramatiq"),
    ("huey", "huey"),
    ("python-rq", "rq"),
    ("rq.Queue", "rq"),
    ("bullmq", "bullmq"),
    ("BullMQ", "bullmq"),
    ("bull.Queue", "bull"),
    ("Sidekiq", "sidekiq"),
    ("sidekiq", "sidekiq"),
    ("Resque", "resque"),
    ("GoodJob", "goodjob"),
    ("DelayedJob", "delayed_job"),
    ("Hangfire", "hangfire"),
    ("NServiceBus", "nservicebus"),
    ("asynq", "asynq"),
    ("RichardKnop/machinery", "machinery"),
    ("temporalio", "temporal"),
    ("@temporalio", "temporal"),
    ("temporal.client", "temporal"),
    ("temporal.worker", "temporal"),
    ("inngest", "inngest"),
    ("Oban", "oban"),
    ("Broadway", "broadway"),
    ("GenStage", "genstage"),
    ("Phoenix.PubSub", "phoenix_pubsub"),
    ("Alpakka", "alpakka"),
    ("mqtt", "mqtt"),
    ("MQTTClient", "mqtt"),
    ("mosquitto", "mqtt"),
    ("rumqttc", "mqtt"),
    ("dapr.clients.grpc", "dapr"),
    ("DaprClient", "dapr"),
];

#[derive(Clone)]
pub(crate) struct SourceFile {
    pub path: ProjectRelativePath,
    pub source: Arc<[u8]>,
}

pub(crate) fn apply_project(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    edges: &mut Vec<GraphEdge>,
    calls: &[ExtractedCall],
    imports: &[ExtractedImport],
    sources: &[SourceFile],
) -> Result<(), IndexError> {
    let import_map = import_map(imports);
    create_environment_edges(project, nodes, edges, calls)?;
    create_service_edges(project, nodes, edges, calls, &import_map)?;
    create_decorator_routes(project, nodes, edges, sources)?;
    create_protocol_handlers(project, nodes, edges, sources)?;
    create_config_links(project, nodes, edges)?;
    create_package_links(project, nodes, edges, imports, sources)?;
    create_data_flows(project, nodes, edges)?;
    nodes.sort_by(|left, right| left.id.cmp(&right.id));
    edges.sort_by(|left, right| {
        (
            &left.source,
            left.kind.as_str(),
            &left.target,
            left.discriminator.as_str(),
        )
            .cmp(&(
                &right.source,
                right.kind.as_str(),
                &right.target,
                right.discriminator.as_str(),
            ))
    });
    Ok(())
}

fn create_environment_edges(
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

fn create_service_edges(
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

fn identifier_fragment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_alphanumeric() || character == '_')
}

fn create_decorator_routes(
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

#[allow(clippy::too_many_lines)]
fn create_protocol_handlers(
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

fn create_config_links(
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

fn create_package_links(
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

fn create_data_flows(
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

fn ensure_node(
    project: &ProjectId,
    nodes: &mut Vec<GraphNode>,
    label: &str,
    name: &str,
    qualified_name: &str,
    file_path: Option<ProjectRelativePath>,
    properties: GraphProperties,
) -> Result<NodeId, IndexError> {
    if let Some(node) = nodes
        .iter()
        .find(|node| node.qualified_name.as_str() == qualified_name)
    {
        return Ok(node.id.clone());
    }
    let synthetic_count = nodes
        .iter()
        .filter(|node| matches!(node.label.as_str(), "Route" | "EnvVar" | "Package"))
        .count();
    if synthetic_count >= MAX_SYNTHETIC_NODES {
        return Err(IndexError::CoordinateOverflow("synthetic node bound"));
    }
    let id = stable_node_id(label, qualified_name)?;
    let node = GraphNode::new(
        project.clone(),
        id.clone(),
        NodeLabel::new(label)?,
        name,
        QualifiedName::new(qualified_name)?,
        file_path,
        None,
        Generation::new(0),
    )?
    .with_properties(properties);
    nodes.push(node);
    Ok(id)
}

fn push_edge(
    project: &ProjectId,
    edges: &mut Vec<GraphEdge>,
    source: &NodeId,
    target: &NodeId,
    kind: &str,
    properties: GraphProperties,
) -> Result<(), IndexError> {
    let synthetic_count = edges
        .iter()
        .filter(|edge| {
            matches!(
                edge.kind.as_str(),
                "HTTP_CALLS" | "ASYNC_CALLS" | "HANDLES" | "DATA_FLOWS" | "CONFIGURES"
            )
        })
        .count();
    if synthetic_count >= MAX_SYNTHETIC_EDGES {
        return Err(IndexError::CoordinateOverflow("synthetic edge bound"));
    }
    if edges
        .iter()
        .any(|edge| &edge.source == source && &edge.target == target && edge.kind.as_str() == kind)
    {
        return Ok(());
    }
    edges.push(
        GraphEdge::new(
            project.clone(),
            source.clone(),
            target.clone(),
            EdgeKind::new(kind)?,
            Generation::new(0),
        )
        .with_properties(properties),
    );
    Ok(())
}

fn stable_node_id(label: &str, qualified_name: &str) -> Result<NodeId, IndexError> {
    let hash = blake3::hash(format!("goldeneye-node-v1\0{label}\0{qualified_name}").as_bytes());
    Ok(NodeId::new(format!(
        "{}:{}",
        label.to_ascii_lowercase(),
        &hash.to_hex()[..32]
    ))?)
}

fn json_properties<const N: usize>(entries: [(&str, Value); N]) -> GraphProperties {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn import_map(imports: &[ExtractedImport]) -> BTreeMap<(ProjectRelativePath, String), String> {
    imports
        .iter()
        .map(|import| {
            (
                (import.file.clone(), import.alias.clone()),
                import.module_path.clone(),
            )
        })
        .collect()
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

fn first_string_literal(text: &str) -> Option<String> {
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

fn http_method(callee: &str) -> Option<&'static str> {
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

fn canonical_route_path(input: &str) -> String {
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

fn route_method_from_annotation(line: &str) -> Option<&'static str> {
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

fn normalize_config_key(value: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::{
        canonical_route_path, first_string_literal, normalize_config_key,
        route_method_from_annotation,
    };

    #[test]
    fn route_paths_canonicalize_framework_parameters() {
        assert_eq!(
            canonical_route_path("/users/:id/posts/{post}"),
            "/users/{}/posts/{}"
        );
        assert_eq!(canonical_route_path("/files/<path:name>"), "/files/{}");
        assert_eq!(canonical_route_path("/jobs/${jobId}"), "/jobs/{}");
    }

    #[test]
    fn literal_and_decorator_helpers_are_bounded_and_deterministic() {
        assert_eq!(
            first_string_literal("client.get(\"/v1/users\")").as_deref(),
            Some("/v1/users")
        );
        assert_eq!(
            route_method_from_annotation("@router.post('/v1/users')"),
            Some("POST")
        );
        assert_eq!(
            normalize_config_key("database.maxConnections"),
            ["database", "max", "connections"]
        );
    }
}
