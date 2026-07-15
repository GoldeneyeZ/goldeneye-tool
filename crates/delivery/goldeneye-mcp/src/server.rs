use std::fs;
use std::sync::{Arc, Mutex};

use goldeneye_bootstrap::BootstrapRuntime;
use goldeneye_services::{
    ArchitectureRequest, CancellationToken, CodeSnippetRequest, CreateFileRequest,
    DeleteNodeRequest, DetectChangesRequest, GraphSchemaRequest, IndexRepositoryMode,
    IndexRepositoryRequest, IndexStatusRequest, IngestTracesRequest, InspectSyntaxRequest,
    ManageAdrRequest, NodeContentRequest, OperationHooks, PageRequest, ProjectId, QueryError,
    QueryGraphRequest, QueryValue, SearchCodeMode, SearchCodeRequest, SearchGraphRequest,
    SemanticSearchRequest, ServiceConfig, ServiceError, ServiceErrorCode, Services, TraceDirection,
    TracePathRequest,
};
use goldeneye_watcher::Watcher;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::protocol::{Request, RequestId, Response};
use crate::tools::{ToolCallResult, ToolRegistry, ToolResponseMode};

mod dispatch;
mod errors;
mod handlers;
#[cfg(test)]
mod tests;

use errors::response_mode_configuration_error;

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
    runtime: BootstrapRuntime,
    response_mode: ToolResponseMode,
    active_index: Mutex<Option<(RequestId, CancellationToken)>>,
}

impl Server {
    #[must_use]
    pub fn new(services: Services) -> Self {
        Self::with_runtime(BootstrapRuntime::new(services))
    }

    #[must_use]
    pub fn with_runtime(runtime: BootstrapRuntime) -> Self {
        Self::with_runtime_and_response_mode(runtime, ToolResponseMode::Dual)
    }

    fn with_runtime_and_response_mode(
        runtime: BootstrapRuntime,
        response_mode: ToolResponseMode,
    ) -> Self {
        Self {
            tools: ToolRegistry::implemented(),
            runtime,
            response_mode,
            active_index: Mutex::new(None),
        }
    }

    #[must_use]
    pub const fn services(&self) -> &Services {
        self.runtime.services()
    }

    #[must_use]
    pub const fn watcher(&self) -> &Arc<Watcher<goldeneye_bootstrap::ServiceIndexer>> {
        self.runtime.watcher()
    }

    /// Builds a server using process environment configuration.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration error when service configuration cannot be resolved.
    pub fn from_env() -> Result<Self, ServiceError> {
        let response_mode =
            ToolResponseMode::from_environment().map_err(response_mode_configuration_error)?;
        BootstrapRuntime::from_env()
            .map(|runtime| Self::with_runtime_and_response_mode(runtime, response_mode))
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
        Some(match self.protocol_result(&request, &id) {
            Ok(Some(value)) => Response::success(id, value),
            Ok(None) => Response::error(Some(id), -32601, "Method not found"),
            Err((code, message)) => Response::error(Some(id), code, message),
        })
    }

    fn protocol_result(
        &self,
        request: &Request,
        id: &RequestId,
    ) -> Result<Option<Value>, (i32, String)> {
        match request.method.as_str() {
            "initialize" => Ok(Some(json!({
                "protocolVersion": negotiated_protocol_version(&request.params),
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": {
                    "name": "codebase-memory-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }))),
            "ping" => Ok(Some(json!({}))),
            "resources/list" => Ok(Some(json!({ "resources": [] }))),
            "resources/templates/list" => Ok(Some(json!({ "resourceTemplates": [] }))),
            "prompts/list" => Ok(Some(json!({ "prompts": [] }))),
            "tools/list" => self.tool_page(&request.params).map(Some),
            "tools/call" => serde_json::to_value(self.call_tool(request, id))
                .map(Some)
                .map_err(|error| (-32603, error.to_string())),
            _ => Ok(None),
        }
    }

    fn tool_page(&self, params: &Value) -> Result<Value, (i32, String)> {
        let cursor = params.get("cursor").and_then(Value::as_str);
        let page = self
            .tools
            .page(cursor)
            .map_err(|error| (-32602, error.to_owned()))?;
        serde_json::to_value(page).map_err(|error| (-32603, error.to_string()))
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
        if name == "detect_changes" {
            return match self.detect_changes(arguments, id) {
                Ok((value, is_error)) => {
                    ToolCallResult::structured_with_mode(value, is_error, self.response_mode)
                }
                Err(message) => ToolCallResult::error(message),
            };
        }
        match self.dispatch(name, arguments, id) {
            Ok(value) => ToolCallResult::success_with_mode(value, self.response_mode),
            Err(message) => ToolCallResult::error(message),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArguments {}

#[derive(Deserialize)]
struct ManageAdrArguments {
    project: Option<String>,
    mode: Option<String>,
    content: Option<String>,
    #[serde(default)]
    sections: Vec<String>,
}

#[derive(Deserialize)]
struct IngestTracesArguments {
    project: Option<String>,
    #[serde(default)]
    traces: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DetectChangesArguments {
    project: Option<String>,
    scope: Option<String>,
    depth: Option<usize>,
    base_branch: Option<String>,
    since: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct IndexArguments {
    repo_path: String,
    mode: Option<String>,
    #[serde(default)]
    persistence: bool,
    target_projects: Option<Vec<String>>,
    name: Option<String>,
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
    semantic_query: Option<Vec<String>>,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchCodeArguments {
    pattern: String,
    project: String,
    file_pattern: Option<String>,
    path_filter: Option<String>,
    #[serde(default)]
    mode: SearchCodeMode,
    #[serde(default)]
    context: usize,
    #[serde(default)]
    regex: bool,
    #[serde(default = "default_search_code_limit")]
    limit: usize,
}

const fn default_search_code_limit() -> usize {
    10
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
