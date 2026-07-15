use std::fs;
use std::sync::Arc;

use super::{LATEST_PROTOCOL_VERSION, Server, response_mode_configuration_error};
use crate::protocol::RequestId;
use crate::tools::ToolResponseMode;
use goldeneye_bootstrap::{BootstrapRuntime, service_dependencies};
use goldeneye_services::{ServiceConfig, ServiceErrorCode, Services};
use serde_json::json;
use tempfile::TempDir;

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
fn explicitly_configured_text_mode_omits_structured_tool_content() {
    let runtime = BootstrapRuntime::from_config(ServiceConfig::default());
    let server = Server::with_runtime_and_response_mode(runtime, ToolResponseMode::Text);
    let response = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}"#,
            )
            .expect("tool response");
    let result = response.result.expect("tool result");

    assert_eq!(result["isError"], false);
    assert!(result.get("structuredContent").is_none());
    let payload = serde_json::from_str::<serde_json::Value>(
        result["content"][0]["text"].as_str().expect("text content"),
    )
    .expect("JSON text content");
    assert!(payload["projects"].is_array());
}

#[test]
fn invalid_response_mode_maps_to_an_exact_configuration_error() {
    let message = "GOLDENEYE_MCP_RESPONSE_MODE must be 'dual' or 'text', got 'invalid'";
    let error = response_mode_configuration_error(message.to_owned());

    assert_eq!(error.code(), ServiceErrorCode::Configuration);
    assert_eq!(error.to_string(), message);
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

    let tools = value["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 21);
    assert_eq!(tools[0]["name"], "index_repository");
    assert!(tools.iter().any(|tool| tool["name"] == "delete_project"));
}

#[test]
fn with_runtime_preserves_the_exact_shared_watcher_registry() {
    let temp = TempDir::new().expect("temp directory");
    let runtime = BootstrapRuntime::from_config(ServiceConfig::new(
        temp.path().join("graph.db"),
        temp.path(),
    ));
    let watcher = Arc::clone(runtime.watcher());
    watcher
        .watch("shared", temp.path())
        .expect("seed shared registry");

    let server = Server::with_runtime(runtime);

    assert!(Arc::ptr_eq(&watcher, server.watcher()));
    assert_eq!(server.watcher().projects().expect("projects").len(), 1);
    let ping = server
        .handle_line(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#)
        .expect("injected runtime protocol response");
    assert_eq!(ping.result, Some(json!({})));
}

#[test]
fn index_repository_applies_project_name_override() {
    let temp = TempDir::new().expect("temp directory");
    let allowed = temp.path().join("allowed");
    let repository = allowed.join("fixture");
    fs::create_dir_all(repository.join("src")).expect("source directory");
    fs::write(repository.join("src/lib.rs"), "pub fn ready() {}\n").expect("source file");
    let server = Server::new(Services::new(
        ServiceConfig::new(temp.path().join("graph.db"), &allowed).with_allowed_root(&allowed),
        service_dependencies(),
    ));
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "index_repository",
            "arguments": {
                "repo_path": repository,
                "name": "Team API",
                "mode": "fast"
            }
        }
    })
    .to_string();

    let response = server.handle_line(&request).expect("request response");
    let value = serde_json::to_value(response).expect("serialize response");

    assert_eq!(value["result"]["structuredContent"]["project"], "Team-API");
    assert_eq!(value["result"]["isError"], false);
    assert_eq!(
        server.watcher().projects().expect("indexed projects").len(),
        1
    );

    let delete = server
            .handle_line(
                r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"delete_project","arguments":{"project":"Team-API"}}}"#,
            )
            .expect("delete response");
    let delete = serde_json::to_value(delete).expect("serialize delete response");
    assert_eq!(delete["result"]["structuredContent"]["status"], "deleted");
    assert!(
        server
            .watcher()
            .projects()
            .expect("deleted projects")
            .is_empty()
    );
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
