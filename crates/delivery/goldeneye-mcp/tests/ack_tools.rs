use std::fs;
use std::path::Path;
use std::sync::Arc;

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_git::GitCommandRepository;
use goldeneye_mcp::server::Server;
use goldeneye_services::{ServiceConfig, ServiceDependencies, Services};
use serde_json::{Value, json};
use tempfile::TempDir;

fn service_dependencies() -> ServiceDependencies {
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
    )
}

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("create fixture source");
    fs::write(
        root.join("src/lib.rs"),
        "pub fn helper() -> usize { 1 }\npub fn entry() -> usize { helper() }\n",
    )
    .expect("write fixture");
    fs::write(
        root.join("src/left.rs"),
        "pub fn duplicate() -> usize { 1 }\n",
    )
    .expect("write left duplicate");
    fs::write(
        root.join("src/right.rs"),
        "pub fn duplicate() -> usize { 2 }\n",
    )
    .expect("write right duplicate");
}

fn server(temp: &TempDir, allowed: &Path) -> Server {
    Server::new(Services::new(
        ServiceConfig::new(temp.path().join("graph.db"), allowed).with_allowed_root(allowed),
        service_dependencies(),
    ))
}

fn request(server: &Server, id: i64, method: &str, params: impl Into<Value>) -> Value {
    let params = params.into();
    let line = serde_json::to_string(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))
    .expect("serialize request");
    serde_json::to_value(server.handle_line(&line).expect("response")).expect("serialize response")
}

fn call(server: &Server, id: i64, name: &str, arguments: impl Into<Value>) -> Value {
    let arguments = arguments.into();
    request(
        server,
        id,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )["result"]
        .clone()
}

fn assert_success(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "tool error: {result}");
    let text: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().expect("text content"))
            .expect("JSON text content");
    assert_eq!(text, result["structuredContent"]);
    &result["structuredContent"]
}

fn assert_query_trace_snippet_architecture(server: &Server, project: &str, qualified_name: &str) {
    let query = call(
        server,
        6,
        "query_graph",
        json!({
            "project": project,
            "query": "MATCH (f:Function) RETURN f.name ORDER BY f.name",
            "max_rows": 20
        }),
    );
    assert!(
        !assert_success(&query)["rows"]
            .as_array()
            .expect("rows")
            .is_empty()
    );
    let trace = call(
        server,
        7,
        "trace_path",
        json!({
            "project": project,
            "function_name": qualified_name,
            "direction": "inbound",
            "depth": 1,
            "mode": "calls"
        }),
    );
    let alias = call(
        server,
        8,
        "trace_call_path",
        json!({
            "project": project,
            "function_name": qualified_name,
            "direction": "inbound",
            "depth": 1,
            "mode": "calls"
        }),
    );
    assert_eq!(assert_success(&trace), assert_success(&alias));

    let snippet = call(
        server,
        9,
        "get_code_snippet",
        json!({"project": project, "qualified_name": qualified_name}),
    );
    assert!(
        assert_success(&snippet)["source"]
            .as_str()
            .expect("source")
            .contains("pub fn helper")
    );
    let architecture = call(
        server,
        10,
        "get_architecture",
        json!({"project": project, "aspects": ["languages", "packages", "entry_points"]}),
    );
    assert!(
        assert_success(&architecture)["total_nodes"]
            .as_u64()
            .expect("node count")
            > 0
    );
}

#[test]
fn registry_is_truthful_and_cursor_paginates_all_ack_tools() {
    let temp = TempDir::new().expect("temp directory");
    let server = server(&temp, temp.path());
    let all = request(&server, 1, "tools/list", json!({}));
    let names = all["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().expect("name"))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "index_repository",
            "list_projects",
            "delete_project",
            "index_status",
            "get_graph_schema",
            "search_graph",
            "search_code",
            "query_graph",
            "trace_path",
            "trace_call_path",
            "get_code_snippet",
            "get_architecture",
            "inspect_syntax",
            "create_file",
            "replace_node",
            "delete_node",
            "insert_before_node",
            "insert_after_node",
            "detect_changes",
            "manage_adr",
            "ingest_traces",
        ]
    );
    assert!(
        all["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .all(|tool| tool["inputSchema"]["type"] == "object"
                && tool["outputSchema"]["type"] == "object")
    );

    let first = request(&server, 2, "tools/list", json!({"cursor": "0"}));
    assert_eq!(first["result"]["tools"].as_array().expect("page").len(), 8);
    assert_eq!(first["result"]["nextCursor"], "8");
    let second = request(&server, 3, "tools/list", json!({"cursor": "8"}));
    assert_eq!(second["result"]["tools"].as_array().expect("page").len(), 8);
    assert_eq!(second["result"]["nextCursor"], "16");
    let third = request(&server, 4, "tools/list", json!({"cursor": "16"}));
    assert_eq!(third["result"]["tools"].as_array().expect("page").len(), 5);
    assert!(third["result"].get("nextCursor").is_none());
}

#[test]
fn index_then_all_ack_read_tools_return_stable_structured_json() {
    let temp = TempDir::new().expect("temp directory");
    let repo = temp.path().join("fixture");
    fixture(&repo);
    let server = server(&temp, temp.path());

    let indexed = call(
        &server,
        1,
        "index_repository",
        json!({"repo_path": repo, "mode": "fast"}),
    );
    let indexed = assert_success(&indexed);
    let project = indexed["project"].as_str().expect("project").to_owned();
    assert_eq!(indexed["status"], "indexed");

    let projects = call(&server, 2, "list_projects", json!({}));
    assert_eq!(assert_success(&projects)["projects"][0]["name"], project);
    let status = call(&server, 3, "index_status", json!({"project": project}));
    assert_eq!(assert_success(&status)["status"], "ready");
    let schema = call(&server, 4, "get_graph_schema", json!({"project": project}));
    assert!(assert_success(&schema)["node_labels"].is_array());

    let search = call(
        &server,
        5,
        "search_graph",
        json!({"project": project, "name_pattern": "^helper$", "limit": 20}),
    );
    let search = assert_success(&search);
    assert!(search["total"].as_u64().expect("search total") > 0);
    let helper = search["results"]
        .as_array()
        .expect("results")
        .iter()
        .find(|row| row["label"] == "Function")
        .expect("helper function");
    let qualified_name = helper["qualified_name"]
        .as_str()
        .expect("qualified name")
        .to_owned();

    let code = call(
        &server,
        13,
        "search_code",
        json!({
            "project": project,
            "pattern": "pub helper",
            "file_pattern": "*.rs",
            "mode": "compact",
            "context": 1,
            "regex": false,
            "limit": 10
        }),
    );
    let code = assert_success(&code);
    assert_eq!(code["total_grep_matches"], 2);
    assert_eq!(code["results"][0]["node"], "helper");
    assert!(code["results"][0]["context"].is_string());

    assert_query_trace_snippet_architecture(&server, &project, &qualified_name);

    let ambiguous = call(
        &server,
        11,
        "get_code_snippet",
        json!({"project": project, "qualified_name": "duplicate"}),
    );
    assert_eq!(ambiguous["isError"], true);
    let ambiguous_text = ambiguous["content"][0]["text"]
        .as_str()
        .expect("ambiguity text");
    assert!(ambiguous_text.contains("candidates:"), "{ambiguous_text}");
    assert!(
        ambiguous_text.contains(".src.left.duplicate"),
        "{ambiguous_text}"
    );
    assert!(
        ambiguous_text.contains(".src.right.duplicate"),
        "{ambiguous_text}"
    );

    let missing = call(
        &server,
        12,
        "get_code_snippet",
        json!({"project": project, "qualified_name": "helpe"}),
    );
    assert_eq!(missing["isError"], true);
    let missing_text = missing["content"][0]["text"]
        .as_str()
        .expect("suggestion text");
    assert!(missing_text.contains("suggestions:"), "{missing_text}");
    assert!(missing_text.contains(".src.lib.helper"), "{missing_text}");
}

#[test]
fn malformed_forbidden_unknown_project_and_unknown_tool_are_tool_errors() {
    let temp = TempDir::new().expect("temp directory");
    let allowed = temp.path().join("allowed");
    let outside = temp.path().join("outside");
    fixture(&allowed.join("inside"));
    fixture(&outside);
    let server = server(&temp, &allowed);

    for result in [
        call(&server, 1, "index_status", json!({"project": 7})),
        call(
            &server,
            2,
            "index_repository",
            json!({"repo_path": outside}),
        ),
        call(&server, 3, "index_status", json!({"project": "missing"})),
        call(&server, 4, "missing", json!({})),
    ] {
        assert_eq!(result["isError"], true, "expected tool error: {result}");
        assert!(
            result["content"][0]["text"]
                .as_str()
                .expect("error text")
                .len()
                > 5
        );
    }
}
