use std::fs;
use std::sync::Mutex;

use goldeneye_services::{
    ArchitectureRequest, CancellationToken, CodeSnippetRequest, CreateFileRequest,
    DeleteNodeRequest, GraphSchemaRequest, IndexRepositoryRequest, IndexStatusRequest,
    InspectSyntaxRequest, NodeContentRequest, OperationHooks, PageRequest, ProjectId, QueryError,
    QueryGraphRequest, QueryValue, SearchGraphRequest, ServiceConfig, ServiceError,
    ServiceErrorCode, Services, TraceDirection, TracePathRequest,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::protocol::{Request, RequestId, Response};
use crate::tools::{ToolCallResult, ToolRegistry};

pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] =
    ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
pub const LATEST_PROTOCOL_VERSION: &str = SUPPORTED_PROTOCOL_VERSIONS[0];

fn negotiated_protocol_version(params: &Value) -> &'static str {
    params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .and_then(|requested| {
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .copied()
                .find(|supported| requested == *supported)
        })
        .unwrap_or(LATEST_PROTOCOL_VERSION)
}

pub struct Server {
    tools: ToolRegistry,
    services: Services,
    active_index: Mutex<Option<(RequestId, CancellationToken)>>,
}

impl Server {
    #[must_use]
    pub fn new(services: Services) -> Self {
        Self {
            tools: ToolRegistry::implemented(),
            services,
            active_index: Mutex::new(None),
        }
    }

    /// Builds a server using process environment configuration.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration error when service configuration cannot be resolved.
    pub fn from_env() -> Result<Self, ServiceError> {
        Ok(Self::new(Services::from_env()?))
    }

    #[must_use]
    pub fn handle_line(&self, line: &str) -> Option<Response> {
        let Ok(request) = Request::parse(line) else {
            return Some(Response::parse_error());
        };
        if request.is_notification() {
            self.handle_notification(&request);
            return None;
        }
        let id = request.id.clone()?;
        let result: Option<Value> = match request.method.as_str() {
            "initialize" => Some(json!({
                "protocolVersion": negotiated_protocol_version(&request.params),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "codebase-memory-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),
            "ping" => Some(json!({})),
            "resources/list" => Some(json!({ "resources": [] })),
            "resources/templates/list" => Some(json!({ "resourceTemplates": [] })),
            "prompts/list" => Some(json!({ "prompts": [] })),
            "tools/list" => {
                let cursor = request.params.get("cursor").and_then(Value::as_str);
                match self.tools.page(cursor) {
                    Ok(page) => match serde_json::to_value(page) {
                        Ok(value) => Some(value),
                        Err(error) => {
                            return Some(Response::error(Some(id), -32603, error.to_string()));
                        }
                    },
                    Err(error) => {
                        return Some(Response::error(Some(id), -32602, error));
                    }
                }
            }
            "tools/call" => match serde_json::to_value(self.call_tool(&request, &id)) {
                Ok(value) => Some(value),
                Err(error) => {
                    return Some(Response::error(Some(id), -32603, error.to_string()));
                }
            },
            _ => None,
        };
        Some(match result {
            Some(value) => Response::success(id, value),
            None => Response::error(Some(id), -32601, "Method not found"),
        })
    }

    fn handle_notification(&self, request: &Request) {
        if request.method != "notifications/cancelled" {
            return;
        }
        let Some(request_id) = request.params.get("requestId") else {
            return;
        };
        let Ok(request_id) = serde_json::from_value::<RequestId>(request_id.clone()) else {
            return;
        };
        if let Ok(active) = self.active_index.lock()
            && let Some((active_id, cancellation)) = active.as_ref()
            && *active_id == request_id
        {
            cancellation.cancel();
        }
    }

    fn call_tool(&self, request: &Request, id: &RequestId) -> ToolCallResult {
        let Some(name) = request.params.get("name").and_then(Value::as_str) else {
            return ToolCallResult::error("Missing tool name");
        };
        let arguments = request
            .params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return ToolCallResult::error(format!(
                "Invalid parameters for {name}: arguments must be an object"
            ));
        }
        match self.dispatch(name, arguments, id) {
            Ok(value) => ToolCallResult::success(value),
            Err(message) => ToolCallResult::error(message),
        }
    }

    fn dispatch(&self, name: &str, arguments: Value, id: &RequestId) -> Result<Value, String> {
        match name {
            "index_repository" => {
                let args: IndexArguments = parse_arguments(name, arguments)?;
                if args.mode.as_deref().is_some_and(|mode| mode != "fast") {
                    return Err(
                        "Invalid parameters for index_repository: only fast mode is supported"
                            .to_owned(),
                    );
                }
                let request = IndexRepositoryRequest::new(args.repo_path);
                self.index_repository(id, &request)
            }
            "list_projects" => {
                let _: EmptyArguments = parse_arguments(name, arguments)?;
                self.list_projects()
            }
            "index_status" => {
                let args: ProjectArguments = parse_arguments(name, arguments)?;
                let project = project_id(name, args.project)?;
                let status = self
                    .services
                    .index_status(&IndexStatusRequest::new(project))
                    .map_err(service_error_message)?;
                to_value(json!({
                    "project": status.project,
                    "root_path": status.root_path,
                    "generation": status.generation,
                    "files": status.files,
                    "nodes": status.nodes,
                    "edges": status.edges,
                    "query_only": status.query_only,
                    "status": "ready"
                }))
            }
            "get_graph_schema" => {
                let args: ProjectArguments = parse_arguments(name, arguments)?;
                let project = project_id(name, args.project)?;
                let schema = self
                    .services
                    .get_graph_schema(&GraphSchemaRequest::new(project))
                    .map_err(service_error_message)?;
                let labels = schema
                    .node_labels
                    .into_iter()
                    .map(|entry| {
                        json!({"label": entry.name, "count": entry.count, "properties": entry.properties})
                    })
                    .collect::<Vec<_>>();
                let edges = schema
                    .edge_types
                    .into_iter()
                    .map(|entry| {
                        json!({"type": entry.name, "count": entry.count, "properties": entry.properties})
                    })
                    .collect::<Vec<_>>();
                Ok(json!({
                    "project": schema.project,
                    "schema_version": schema.schema_version,
                    "node_labels": labels,
                    "edge_types": edges
                }))
            }
            "search_graph" => {
                let args: SearchArguments = parse_arguments(name, arguments)?;
                self.search_graph(args)
            }
            "query_graph" => {
                let args: QueryArguments = parse_arguments(name, arguments)?;
                self.query_graph(args)
            }
            "trace_path" | "trace_call_path" => {
                let args: TraceArguments = parse_arguments(name, arguments)?;
                self.trace_path(args)
            }
            "get_code_snippet" => {
                let args: SnippetArguments = parse_arguments(name, arguments)?;
                self.get_code_snippet(args)
            }
            "get_architecture" => {
                let args: ArchitectureArguments = parse_arguments(name, arguments)?;
                self.get_architecture(args)
            }
            "inspect_syntax" => {
                let request: InspectSyntaxRequest = parse_arguments(name, arguments)?;
                to_value(
                    self.services
                        .inspect_syntax(&request)
                        .map_err(service_error_message)?,
                )
            }
            "create_file" => {
                let request: CreateFileRequest = parse_arguments(name, arguments)?;
                to_value(
                    self.services
                        .create_file(&request)
                        .map_err(service_error_message)?,
                )
            }
            "replace_node" => {
                let request: NodeContentRequest = parse_arguments(name, arguments)?;
                to_value(
                    self.services
                        .replace_node(&request)
                        .map_err(service_error_message)?,
                )
            }
            "delete_node" => {
                let request: DeleteNodeRequest = parse_arguments(name, arguments)?;
                to_value(
                    self.services
                        .delete_node(&request)
                        .map_err(service_error_message)?,
                )
            }
            "insert_before_node" => {
                let request: NodeContentRequest = parse_arguments(name, arguments)?;
                to_value(
                    self.services
                        .insert_before_node(&request)
                        .map_err(service_error_message)?,
                )
            }
            "insert_after_node" => {
                let request: NodeContentRequest = parse_arguments(name, arguments)?;
                to_value(
                    self.services
                        .insert_after_node(&request)
                        .map_err(service_error_message)?,
                )
            }
            _ => Err(format!("Unknown tool: {name}")),
        }
    }

    fn index_repository(
        &self,
        id: &RequestId,
        request: &IndexRepositoryRequest,
    ) -> Result<Value, String> {
        let cancellation = CancellationToken::new();
        {
            let mut active = self
                .active_index
                .lock()
                .map_err(|_| "index cancellation state is unavailable".to_owned())?;
            *active = Some((id.clone(), cancellation.clone()));
        }
        let result = self
            .services
            .index_repository_with_hooks(request, &OperationHooks::new(cancellation));
        if let Ok(mut active) = self.active_index.lock()
            && active
                .as_ref()
                .is_some_and(|(active_id, _)| active_id == id)
        {
            *active = None;
        }
        to_value(result.map_err(service_error_message)?)
    }

    fn list_projects(&self) -> Result<Value, String> {
        let projects = self
            .services
            .list_projects()
            .map_err(service_error_message)?;
        let database_bytes = fs::metadata(self.services.config().database_path())
            .map_or(0, |metadata| metadata.len());
        let mut rows = Vec::with_capacity(projects.len());
        for project in projects {
            let id = ProjectId::new(project.project.clone())
                .map_err(|error| format!("stored project name is invalid: {error}"))?;
            let status = self
                .services
                .index_status(&IndexStatusRequest::new(id))
                .map_err(service_error_message)?;
            rows.push(json!({
                "name": project.project,
                "root_path": project.root_path,
                "generation": project.generation,
                "nodes": status.nodes,
                "edges": status.edges,
                "size_bytes": database_bytes
            }));
        }
        Ok(json!({"projects": rows}))
    }

    fn search_graph(&self, args: SearchArguments) -> Result<Value, String> {
        let project = project_id("search_graph", args.project)?;
        let search_mode = if args.query.is_some() {
            "bm25"
        } else {
            "regex"
        };
        let mut request = SearchGraphRequest::new(project);
        request.query = args.query;
        request.name_pattern = args.name_pattern;
        request.qualified_name_pattern = args.qn_pattern;
        request.label = args.label;
        request.file_pattern = args.file_pattern;
        request.relationship = args.relationship;
        request.min_degree = args.min_degree;
        request.max_degree = args.max_degree;
        request.exclude_entry_points = args.exclude_entry_points;
        request.include_connected = args.include_connected;
        request.page = PageRequest {
            limit: args.limit,
            offset: args.offset,
            cursor: args.cursor,
        };
        let page = self
            .services
            .search_graph(&request)
            .map_err(service_error_message)?;
        let mut value = to_value(page)?;
        value
            .as_object_mut()
            .ok_or_else(|| "search serialization did not produce an object".to_owned())?
            .insert(
                "search_mode".to_owned(),
                Value::String(search_mode.to_owned()),
            );
        Ok(value)
    }

    fn query_graph(&self, args: QueryArguments) -> Result<Value, String> {
        let project = project_id("query_graph", args.project)?;
        let mut request = QueryGraphRequest::new(project, args.query);
        request.max_rows = args.max_rows;
        let result = self
            .services
            .query_graph(&request)
            .map_err(service_error_message)?;
        let rows = result
            .rows
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(query_value)
                    .collect::<Result<Vec<_>, _>>()
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({
            "project": result.project,
            "columns": result.columns,
            "rows": rows,
            "total": result.total,
            "truncated": result.truncated
        }))
    }

    fn trace_path(&self, args: TraceArguments) -> Result<Value, String> {
        if args.mode.as_deref().is_some_and(|mode| mode != "calls") {
            return Err(
                "Invalid parameters for trace_path: only calls mode is supported".to_owned(),
            );
        }
        let project = project_id("trace_path", args.project)?;
        let mut request = TracePathRequest::new(project, args.function_name, args.direction);
        request.depth = args.depth;
        request.edge_types = args.edge_types;
        let result = self
            .services
            .trace_path(&request)
            .map_err(service_error_message)?;
        let mut value = to_value(&result)?;
        let paths = to_value(&result.paths)?;
        let object = value
            .as_object_mut()
            .ok_or_else(|| "trace serialization did not produce an object".to_owned())?;
        match result.direction {
            TraceDirection::Inbound => {
                object.insert("callers".to_owned(), paths);
            }
            TraceDirection::Outbound => {
                object.insert("callees".to_owned(), paths);
            }
            TraceDirection::Both => {
                object.insert("callers".to_owned(), paths.clone());
                object.insert("callees".to_owned(), paths);
            }
        }
        Ok(value)
    }

    fn get_code_snippet(&self, args: SnippetArguments) -> Result<Value, String> {
        let project = project_id("get_code_snippet", args.project)?;
        let result = self
            .services
            .get_code_snippet(&CodeSnippetRequest::new(project, args.qualified_name))
            .map_err(service_error_message)?;
        let Value::Object(mut object) = to_value(&result.symbol)? else {
            return Err("snippet symbol serialization did not produce an object".to_owned());
        };
        object.insert("project".to_owned(), Value::String(result.project));
        object.insert("source".to_owned(), Value::String(result.source.clone()));
        object.insert("code".to_owned(), Value::String(result.source));
        object.insert("file_path".to_owned(), Value::String(result.file_path));
        object.insert("start_byte".to_owned(), json!(result.start_byte));
        object.insert("end_byte".to_owned(), json!(result.end_byte));
        object.insert("start_line".to_owned(), json!(result.start_line));
        object.insert("end_line".to_owned(), json!(result.end_line));
        object.insert(
            "content_hash".to_owned(),
            Value::String(result.content_hash),
        );
        Ok(Value::Object(object))
    }

    fn get_architecture(&self, args: ArchitectureArguments) -> Result<Value, String> {
        let project = project_id("get_architecture", args.project)?;
        let result = self
            .services
            .get_architecture(&ArchitectureRequest::new(project))
            .map_err(service_error_message)?;
        Ok(json!({
            "project": result.project,
            "root_path": result.root_path,
            "generation": result.generation,
            "total_nodes": result.total_nodes,
            "total_edges": result.total_edges,
            "languages": result.languages,
            "packages": result.modules,
            "types": result.types,
            "entry_points": result.entry_points,
            "edge_types": result.edge_types,
            "hotspots": [],
            "boundaries": [],
            "layers": [],
            "clusters": []
        }))
    }
}

impl Default for Server {
    fn default() -> Self {
        Self::new(Services::new(ServiceConfig::default()))
    }
}

fn parse_arguments<T: DeserializeOwned>(name: &str, value: Value) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("Invalid parameters for {name}: {error}"))
}

fn project_id(tool: &str, project: String) -> Result<ProjectId, String> {
    ProjectId::new(project)
        .map_err(|error| format!("Invalid parameters for {tool}: invalid project: {error}"))
}

fn service_error_message(error: ServiceError) -> String {
    match error {
        ServiceError::Query(QueryError::ProjectNotFound(project)) => {
            format!("project not found or not indexed: {}", project.as_str())
        }
        ServiceError::Query(QueryError::AmbiguousSymbol {
            query,
            mut candidates,
        }) => {
            candidates.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            let names = candidates
                .into_iter()
                .map(|candidate| candidate.qualified_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("symbol is ambiguous: {query}; candidates: {names}")
        }
        ServiceError::Query(QueryError::SymbolNotFound {
            query,
            mut suggestions,
        }) => {
            suggestions.sort_by(|left, right| left.qualified_name.cmp(&right.qualified_name));
            let names = suggestions
                .into_iter()
                .map(|suggestion| suggestion.qualified_name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("symbol was not found: {query}; suggestions: {names}")
        }
        ServiceError::OutsideAllowedRoot => "repo_path is outside the allowed root".to_owned(),
        ServiceError::Cancelled => "Request cancelled".to_owned(),
        ServiceError::Edit { code, message } => {
            format!("{}: {message}", service_error_code(code))
        }
        other => other.to_string(),
    }
}

const fn service_error_code(code: ServiceErrorCode) -> &'static str {
    match code {
        ServiceErrorCode::Configuration => "configuration",
        ServiceErrorCode::InvalidInput => "invalid_input",
        ServiceErrorCode::Forbidden => "forbidden",
        ServiceErrorCode::NotFound => "not_found",
        ServiceErrorCode::Cancelled => "cancelled",
        ServiceErrorCode::Storage => "storage",
        ServiceErrorCode::Index => "index",
        ServiceErrorCode::Query => "query",
        ServiceErrorCode::Conflict => "conflict",
    }
}

fn to_value(value: impl Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|error| format!("result serialization failed: {error}"))
}

fn query_value(value: QueryValue) -> Result<Value, String> {
    match value {
        QueryValue::Null => Ok(Value::Null),
        QueryValue::Bool(value) => Ok(Value::Bool(value)),
        QueryValue::Integer(value) => Ok(json!(value)),
        QueryValue::Unsigned(value) => Ok(json!(value)),
        QueryValue::Float(value) => Ok(json!(value)),
        QueryValue::String(value) => Ok(Value::String(value)),
        QueryValue::Node(value) => to_value(value),
        QueryValue::Edge(value) => to_value(value),
        QueryValue::Json(value) => Ok(value),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArguments {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexArguments {
    repo_path: String,
    mode: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectArguments {
    project: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    project: String,
    query: Option<String>,
    name_pattern: Option<String>,
    qn_pattern: Option<String>,
    label: Option<String>,
    file_pattern: Option<String>,
    relationship: Option<String>,
    min_degree: Option<usize>,
    max_degree: Option<usize>,
    #[serde(default)]
    exclude_entry_points: bool,
    #[serde(default)]
    include_connected: bool,
    #[serde(default = "default_search_limit")]
    limit: usize,
    #[serde(default)]
    offset: usize,
    cursor: Option<String>,
}

const fn default_search_limit() -> usize {
    20
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryArguments {
    project: String,
    query: String,
    #[serde(default = "default_query_rows")]
    max_rows: usize,
}

const fn default_query_rows() -> usize {
    200
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TraceArguments {
    project: String,
    function_name: String,
    #[serde(default = "default_trace_direction")]
    direction: TraceDirection,
    #[serde(default = "default_trace_depth")]
    depth: usize,
    #[serde(default = "default_edge_types")]
    edge_types: Vec<String>,
    mode: Option<String>,
}

const fn default_trace_direction() -> TraceDirection {
    TraceDirection::Both
}

const fn default_trace_depth() -> usize {
    1
}

fn default_edge_types() -> Vec<String> {
    vec!["CALLS".to_owned()]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnippetArguments {
    project: String,
    qualified_name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchitectureArguments {
    project: String,
    #[serde(default, rename = "aspects")]
    _aspects: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::{LATEST_PROTOCOL_VERSION, Server};
    use crate::protocol::RequestId;
    use serde_json::json;

    #[test]
    fn initialize_returns_upstream_identity_and_latest_protocol() {
        let response = Server::default()
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#)
            .expect("request response");
        let value = serde_json::to_value(response).expect("serialize response");
        assert_eq!(value["result"]["protocolVersion"], "2025-11-25");
        assert_eq!(value["result"]["serverInfo"]["name"], "codebase-memory-mcp");
        assert_eq!(
            value["result"]["capabilities"]["tools"]["listChanged"],
            false
        );
    }

    #[test]
    fn initialize_echoes_every_supported_protocol_version() {
        for version in ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"] {
            let request = format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{version}"}}}}"#
            );
            let response = Server::default()
                .handle_line(&request)
                .expect("request response");

            assert_eq!(
                response.result.expect("initialize result")["protocolVersion"],
                version
            );
        }
    }

    #[test]
    fn initialize_falls_back_to_latest_for_unsupported_version() {
        let response = Server::default()
            .handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"unsupported"}}"#,
            )
            .expect("request response");

        assert_eq!(
            response.result.expect("initialize result")["protocolVersion"],
            LATEST_PROTOCOL_VERSION
        );
    }

    #[test]
    fn parse_failures_use_stable_upstream_error() {
        for input in ["{", "[]", r#"{"jsonrpc":"2.0","id":1}"#] {
            let response = Server::default()
                .handle_line(input)
                .expect("parse response");
            let error = response.error.expect("parse error");

            assert_eq!(response.id, Some(RequestId::Number(0)));
            assert_eq!(error.code, -32700);
            assert_eq!(error.message, "Parse error");
        }
    }

    #[test]
    fn invalid_json_and_unknown_method_use_jsonrpc_errors() {
        let server = Server::default();
        let parse = server.handle_line("{").expect("parse error response");
        let unknown = server
            .handle_line(r#"{"jsonrpc":"2.0","id":"x","method":"missing"}"#)
            .expect("method error response");
        assert_eq!(parse.error.expect("parse error").code, -32700);
        assert_eq!(unknown.error.expect("method error").code, -32601);
    }

    #[test]
    fn notifications_return_no_response() {
        let response = Server::default().handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        );
        assert!(response.is_none());
    }

    #[test]
    fn lifecycle_list_and_ping_methods_return_empty_results() {
        let server = Server::default();
        let cases = [
            ("ping", serde_json::json!({})),
            ("resources/list", serde_json::json!({ "resources": [] })),
            (
                "resources/templates/list",
                serde_json::json!({ "resourceTemplates": [] }),
            ),
            ("prompts/list", serde_json::json!({ "prompts": [] })),
        ];

        for (method, expected) in cases {
            let request = format!(r#"{{"jsonrpc":"2.0","id":1,"method":"{method}"}}"#);
            let response = server.handle_line(&request).expect("request response");
            let value = serde_json::to_value(response).expect("serialize response");
            assert_eq!(value["result"], expected, "method {method}");
        }
    }

    #[test]
    fn tools_list_truthfully_advertises_implemented_tools() {
        let response = Server::default()
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .expect("request response");
        let value = serde_json::to_value(response).expect("serialize response");

        assert_eq!(
            value["result"]["tools"].as_array().expect("tools").len(),
            10
        );
        assert_eq!(value["result"]["tools"][0]["name"], "index_repository");
    }

    #[test]
    fn unknown_tool_call_returns_mcp_error_result_envelope() {
        let response = Server::default()
            .handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"missing","arguments":{}}}"#,
            )
            .expect("request response");
        let value = serde_json::to_value(response).expect("serialize response");

        assert_eq!(
            value["result"],
            json!({
                "content": [{"type": "text", "text": "Unknown tool: missing"}],
                "isError": true
            })
        );
        assert!(value.get("error").is_none());
    }
}
