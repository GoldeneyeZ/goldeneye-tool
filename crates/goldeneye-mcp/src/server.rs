use crate::protocol::{Request, Response};
use crate::tools::{ToolCallResult, ToolRegistry};
use serde_json::{Value, json};

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

#[derive(Default)]
pub struct Server {
    tools: ToolRegistry,
}

impl Server {
    #[must_use]
    pub fn handle_line(&self, line: &str) -> Option<Response> {
        let Ok(request) = Request::parse(line) else {
            return Some(Response::parse_error());
        };
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
            "tools/call" => {
                let name = request
                    .params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("missing");
                match serde_json::to_value(ToolCallResult::error(format!("Unknown tool: {name}"))) {
                    Ok(value) => Some(value),
                    Err(error) => {
                        return Some(Response::error(Some(id), -32603, error.to_string()));
                    }
                }
            }
            _ => None,
        };
        Some(match result {
            Some(value) => Response::success(id, value),
            None => Response::error(Some(id), -32601, "Method not found"),
        })
    }
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
    fn tools_list_truthfully_advertises_no_unimplemented_tools() {
        let response = Server::default()
            .handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#)
            .expect("request response");
        let value = serde_json::to_value(response).expect("serialize response");

        assert_eq!(value["result"], json!({"tools": []}));
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
