use std::fs;
use std::path::Path;
use std::sync::Arc;

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_git::GitCommandRepository;
use goldeneye_mcp::server::Server;
use goldeneye_mcp::tools::ToolRegistry;
use goldeneye_services::{IndexRepositoryRequest, ServiceConfig, ServiceDependencies, Services};
use goldeneye_store::SqliteRepositoryFactory;
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
use serde_json::{Value, json};
use tempfile::TempDir;

fn service_dependencies() -> ServiceDependencies {
    let discovery = Arc::new(FileSystemDiscovery);
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
        discovery,
        Arc::new(SqliteRepositoryFactory),
        Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
        Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
    )
}

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("source directory");
    fs::write(root.join("src/lib.rs"), "pub fn entry() -> usize { 1 }\n").expect("source file");
}

fn indexed_server(temp: &TempDir) -> (Server, String) {
    let root = temp.path().join("project");
    fixture(&root);
    let services = Services::new(
        ServiceConfig::new(temp.path().join("graph.sqlite3"), &root).with_allowed_root(temp.path()),
        service_dependencies(),
    );
    let indexed = services
        .index_repository(&IndexRepositoryRequest::new(&root))
        .expect("index fixture");
    (Server::new(services), indexed.project)
}

#[allow(clippy::needless_pass_by_value)]
fn call(server: &Server, id: i64, name: &str, arguments: Value) -> Value {
    let line = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": name, "arguments": arguments}
    })
    .to_string();
    serde_json::to_value(server.handle_line(&line).expect("response"))
        .expect("response JSON")["result"]
        .clone()
}

fn successful(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "tool error: {result}");
    let text: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().expect("text result"))
            .expect("JSON text");
    assert_eq!(text, result["structuredContent"]);
    &result["structuredContent"]
}

#[test]
fn registry_exposes_exact_upstream_adr_and_trace_schemas() {
    let registry = ToolRegistry::implemented();
    let all = registry.page(None).expect("all tools");
    assert_eq!(all.tools.len(), 21);
    let manage = all
        .tools
        .iter()
        .find(|tool| tool.name == "manage_adr")
        .expect("manage_adr");
    assert_eq!(manage.title, "Manage ADR");
    assert_eq!(
        manage.input_schema,
        json!({
            "type": "object",
            "properties": {
                "project": {"type": "string"},
                "mode": {"type": "string", "enum": ["get", "update", "sections"]},
                "content": {"type": "string"},
                "sections": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["project"]
        })
    );
    let ingest = all
        .tools
        .iter()
        .find(|tool| tool.name == "ingest_traces")
        .expect("ingest_traces");
    assert_eq!(ingest.title, "Ingest traces");
    assert_eq!(
        ingest.input_schema,
        json!({
            "type": "object",
            "properties": {
                "traces": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "caller": {"type": "string"},
                            "callee": {"type": "string"},
                            "count": {"type": "integer"}
                        },
                        "additionalProperties": false
                    }
                },
                "project": {"type": "string"}
            },
            "required": ["traces", "project"]
        })
    );

    assert_eq!(registry.page(Some("0")).expect("first page").tools.len(), 8);
    assert_eq!(
        registry.page(Some("8")).expect("second page").tools.len(),
        8
    );
    assert_eq!(registry.page(Some("16")).expect("last page").tools.len(), 5);
}

#[test]
fn manage_adr_roundtrips_all_upstream_modes_and_rich_errors() {
    let temp = TempDir::new().expect("temp dir");
    let (server, project) = indexed_server(&temp);

    assert_eq!(
        successful(&call(&server, 1, "manage_adr", json!({"project": project}))),
        &json!({
            "content": "",
            "status": "no_adr",
            "adr_hint": goldeneye_services::ADR_EMPTY_HINT
        })
    );
    assert_eq!(
        successful(&call(
            &server,
            2,
            "manage_adr",
            json!({
                "project": project,
                "mode": "update",
                "content": "# Architecture\r\n## PURPOSE\nMCP"
            })
        )),
        &json!({"status": "updated"})
    );
    assert_eq!(
        successful(&call(
            &server,
            3,
            "manage_adr",
            json!({"project": project, "mode": "get"})
        )),
        &json!({"content": "# Architecture\r\n## PURPOSE\nMCP"})
    );
    assert_eq!(
        successful(&call(
            &server,
            4,
            "manage_adr",
            json!({"project": project, "mode": "sections", "sections": ["ignored"]})
        )),
        &json!({"sections": ["# Architecture", "## PURPOSE"]})
    );

    let missing = call(&server, 5, "manage_adr", json!({}));
    assert_eq!(missing["isError"], true);
    let missing_text = missing["content"][0]["text"]
        .as_str()
        .expect("missing project error");
    assert!(missing_text.contains("missing required argument: project"));
    assert!(missing_text.contains("hint"));

    let unknown = call(
        &server,
        6,
        "manage_adr",
        json!({"project": "not-indexed", "mode": "get"}),
    );
    assert_eq!(unknown["isError"], true);
    let unknown_text = unknown["content"][0]["text"]
        .as_str()
        .expect("unknown project error");
    assert!(unknown_text.contains("project not found or not indexed"));
    assert!(unknown_text.contains("hint"));
    assert!(unknown_text.contains(&project));
}

#[test]
fn ingest_traces_preserves_permissive_upstream_envelope() {
    let temp = TempDir::new().expect("temp dir");
    let (server, project) = indexed_server(&temp);
    let expected = json!({
        "status": "accepted",
        "traces_received": 3,
        "note": "Runtime edge creation from traces not yet implemented"
    });
    assert_eq!(
        successful(&call(
            &server,
            7,
            "ingest_traces",
            json!({
                "project": project,
                "traces": [
                    {"caller": "a", "callee": "b"},
                    {"caller": "a"},
                    {"caller": "a", "callee": "b", "count": 4, "ignored": true}
                ]
            })
        )),
        &expected
    );
    assert_eq!(
        successful(&call(&server, 8, "ingest_traces", json!({}))),
        &json!({
            "status": "accepted",
            "traces_received": 0,
            "note": "Runtime edge creation from traces not yet implemented"
        })
    );
}
