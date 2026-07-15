use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::Arc;

use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_edit::{
    DurableEditRequest, DurableEditService, EditOperation, EditOptions, FaultInjector, FaultPoint,
};
use goldeneye_index::{IndexOptions, IndexService};
use goldeneye_mcp::server::Server;
use goldeneye_services::NodeLocator;
use goldeneye_store::Store;
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use serde_json::{Value, json};
use tempfile::TempDir;

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("create source directory");
    fs::write(
        root.join("src/lib.rs"),
        "pub fn helper() -> usize { 1 }\npub fn entry() -> usize { helper() }\n",
    )
    .expect("write fixture");
}

fn run_server(input: &[u8], database: &Path, root: &Path) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_goldeneye"))
        .env("GOLDENEYE_DB_PATH", database)
        .env("GOLDENEYE_PROJECT_ROOT", root)
        .env("CBM_ALLOWED_ROOT", root)
        .env("CBM_SEMANTIC_ENABLED", "1")
        .env("CBM_SEMANTIC_THRESHOLD", "0.82")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn goldeneye");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write requests");
    child.wait_with_output().expect("wait for goldeneye")
}

fn responses(output: &Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr must contain no protocol output: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::str::from_utf8(&output.stdout)
        .expect("UTF-8 stdout")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSON-only stdout"))
        .collect()
}

#[allow(clippy::needless_pass_by_value)]
fn call_tool(database: &Path, root: &Path, id: u64, name: &str, arguments: Value) -> Value {
    let input = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        })
    );
    responses(&run_server(input.as_bytes(), database, root))
        .into_iter()
        .next()
        .expect("one response")["result"]
        .clone()
}

fn successful_content(result: &Value) -> &Value {
    assert_eq!(result["isError"], false, "tool failed: {result}");
    let text: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().expect("text content"))
            .expect("JSON text content");
    assert_eq!(text, result["structuredContent"]);
    &result["structuredContent"]
}

fn locator_with_preview(inspection: &Value, needle: &str) -> Value {
    let nodes = inspection["syntax"]["n"].as_array().expect("syntax nodes");
    let index = nodes
        .iter()
        .position(|node| {
            node["k"] == "function_item"
                && node["v"]
                    .as_str()
                    .is_some_and(|preview| preview.contains(needle))
        })
        .expect("matching function node");
    inspection["locators"][index].clone()
}

fn inspect_file(database: &Path, root: &Path, project: &str, id: u64) -> Value {
    let result = call_tool(
        database,
        root,
        id,
        "inspect_syntax",
        json!({
            "project": project,
            "path": "src/lib.rs",
            "inspect": {
                "max_depth": 8,
                "max_nodes": 200,
                "preview_chars": 128,
                "node_kinds": []
            }
        }),
    );
    successful_content(&result).clone()
}

#[test]
fn injected_server_session_preserves_foundation_protocol() {
    let server = Server::default();
    let mut output = Vec::new();
    goldeneye::run_session_with_server(
        Cursor::new(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n"),
        &mut output,
        &server,
    )
    .expect("injected session");
    assert_eq!(
        serde_json::from_slice::<Value>(&output).expect("JSON response"),
        json!({"jsonrpc": "2.0", "id": 1, "result": {}})
    );
}

#[test]
fn stdio_indexes_then_reopens_persistent_services_with_clean_streams() {
    let temp = TempDir::new().expect("temp directory");
    let repo = temp.path().join("fixture");
    fixture(&repo);
    let database = temp.path().join("state/graph.db");
    let index_request = format!(
        "{}\n",
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": "index_repository",
                "arguments": {"repo_path": repo, "mode": "fast"}
            }
        })
    );
    let first = responses(&run_server(
        index_request.as_bytes(),
        &database,
        temp.path(),
    ));
    let project = first[0]["result"]["structuredContent"]["project"]
        .as_str()
        .expect("project")
        .to_owned();

    let mut second_input = String::new();
    for value in [
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"index_status","arguments":{"project":project}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"search_graph","arguments":{"project":project,"query":"helper","limit":5}}}),
        json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"search_code","arguments":{"project":project,"pattern":"pub helper","mode":"compact","limit":5}}}),
    ] {
        second_input.push_str(&value.to_string());
        second_input.push('\n');
    }
    let second = responses(&run_server(second_input.as_bytes(), &database, temp.path()));
    assert_eq!(second.len(), 5);
    let tools = second[0]["result"]["tools"].as_array().expect("tools");
    assert_eq!(tools.len(), 21);
    assert!(tools.iter().any(|tool| tool["name"] == "delete_project"));
    assert_eq!(
        second[1]["result"]["structuredContent"]["projects"][0]["name"],
        project
    );
    assert_eq!(second[2]["result"]["structuredContent"]["status"], "ready");
    assert!(
        second[3]["result"]["structuredContent"]["total"]
            .as_u64()
            .expect("search total")
            > 0
    );
    assert_eq!(
        second[4]["result"]["structuredContent"]["total_grep_matches"],
        2
    );
    assert_eq!(
        second[4]["result"]["structuredContent"]["results"][0]["node"],
        "helper"
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn stdio_structural_edit_tools_roundtrip_locators_and_refresh_ack_reads() {
    let temp = TempDir::new().expect("temp directory");
    let repo = temp.path().join("fixture");
    fixture(&repo);
    let database = temp.path().join("state/graph.db");

    let indexed = call_tool(
        &database,
        temp.path(),
        1,
        "index_repository",
        json!({"repo_path": repo, "mode": "fast"}),
    );
    let indexed = successful_content(&indexed);
    let project = indexed["project"].as_str().expect("project").to_owned();

    let first = inspect_file(&database, temp.path(), &project, 2);
    let stale_helper = locator_with_preview(&first, "fn helper");
    let replaced = call_tool(
        &database,
        temp.path(),
        3,
        "replace_node",
        json!({
            "operation_id": "stdio-replace",
            "locator": stale_helper,
            "content": "pub fn helper() -> usize { 2 }",
            "parse_policy": "require_clean"
        }),
    );
    let replaced = successful_content(&replaced);
    assert!(
        !replaced["changed_syntax_ids"]
            .as_array()
            .expect("syntax IDs")
            .is_empty()
    );
    assert!(
        !replaced["changed_graph_ids"]
            .as_array()
            .expect("graph IDs")
            .is_empty()
    );

    let stale_bytes = fs::read(repo.join("src/lib.rs")).expect("source after replace");
    let stale = call_tool(
        &database,
        temp.path(),
        4,
        "replace_node",
        json!({
            "operation_id": "stdio-stale",
            "locator": stale_helper,
            "content": "pub fn helper() -> usize { 99 }"
        }),
    );
    assert_eq!(stale["isError"], true);
    assert!(
        stale["content"][0]["text"]
            .as_str()
            .expect("stale error")
            .contains("fresh_syntax=")
    );
    assert_eq!(
        fs::read(repo.join("src/lib.rs")).expect("source after stale rejection"),
        stale_bytes
    );

    let second = inspect_file(&database, temp.path(), &project, 5);
    let helper = locator_with_preview(&second, "fn helper");
    let inserted_before = call_tool(
        &database,
        temp.path(),
        6,
        "insert_before_node",
        json!({
            "operation_id": "stdio-insert-before",
            "locator": helper,
            "content": "pub fn injected() -> usize { 9 }\n"
        }),
    );
    successful_content(&inserted_before);

    let third = inspect_file(&database, temp.path(), &project, 7);
    let injected = locator_with_preview(&third, "fn injected");
    let deleted = call_tool(
        &database,
        temp.path(),
        8,
        "delete_node",
        json!({"operation_id": "stdio-delete", "locator": injected}),
    );
    successful_content(&deleted);

    let fourth = inspect_file(&database, temp.path(), &project, 9);
    let helper = locator_with_preview(&fourth, "fn helper");
    let inserted_after = call_tool(
        &database,
        temp.path(),
        10,
        "insert_after_node",
        json!({
            "operation_id": "stdio-insert-after",
            "locator": helper,
            "content": "\npub fn after_helper() -> usize { 8 }"
        }),
    );
    let inserted_after = successful_content(&inserted_after);

    let created = call_tool(
        &database,
        temp.path(),
        11,
        "create_file",
        json!({
            "operation_id": "stdio-create",
            "project": project,
            "path": "src/nested/extra.rs",
            "content": "pub fn extra() -> usize { 3 }\n",
            "expected_generation": inserted_after["generation"],
            "parse_policy": "require_clean",
            "create_parents": true
        }),
    );
    let created = successful_content(&created);
    assert!(repo.join("src/nested/extra.rs").is_file());

    let existing = call_tool(
        &database,
        temp.path(),
        12,
        "create_file",
        json!({
            "operation_id": "stdio-create-existing",
            "project": project,
            "path": "src/nested/extra.rs",
            "content": "overwrite",
            "expected_generation": created["generation"],
            "create_parents": true
        }),
    );
    assert_eq!(existing["isError"], true);

    let escaped = call_tool(
        &database,
        temp.path(),
        13,
        "create_file",
        json!({
            "operation_id": "stdio-create-escape",
            "project": project,
            "path": "../escape.rs",
            "content": "escape",
            "expected_generation": created["generation"]
        }),
    );
    assert_eq!(escaped["isError"], true);
    assert!(!temp.path().join("escape.rs").exists());

    let helper_search = call_tool(
        &database,
        temp.path(),
        14,
        "search_graph",
        json!({"project": project, "name_pattern": "^helper$", "limit": 20}),
    );
    let helper_search = successful_content(&helper_search);
    let helper = helper_search["results"]
        .as_array()
        .expect("helper results")
        .iter()
        .find(|row| row["label"] == "Function")
        .expect("helper function");
    let qualified = helper["qualified_name"].as_str().expect("qualified name");
    let snippet = call_tool(
        &database,
        temp.path(),
        15,
        "get_code_snippet",
        json!({"project": project, "qualified_name": qualified}),
    );
    assert!(
        successful_content(&snippet)["source"]
            .as_str()
            .expect("source")
            .contains("{ 2 }")
    );
    let trace = call_tool(
        &database,
        temp.path(),
        16,
        "trace_path",
        json!({
            "project": project,
            "function_name": qualified,
            "direction": "inbound",
            "depth": 1,
            "mode": "calls"
        }),
    );
    assert!(
        !successful_content(&trace)["callers"]
            .as_array()
            .expect("callers")
            .is_empty()
    );

    let created_search = call_tool(
        &database,
        temp.path(),
        17,
        "search_graph",
        json!({"project": project, "name_pattern": "^extra$", "limit": 20}),
    );
    assert!(
        successful_content(&created_search)["total"]
            .as_u64()
            .expect("created search total")
            > 0
    );
}

#[derive(Debug)]
struct FailAfterRename;

impl FaultInjector for FailAfterRename {
    fn check(&self, point: FaultPoint) -> Result<(), String> {
        if point == FaultPoint::AfterRename {
            Err("simulated process interruption".to_owned())
        } else {
            Ok(())
        }
    }
}

#[test]
fn stdio_startup_recovers_interrupted_edit_before_first_response() {
    let temp = TempDir::new().expect("temp directory");
    let repo = temp.path().join("fixture");
    fixture(&repo);
    let database = temp.path().join("state/graph.db");
    let indexed = call_tool(
        &database,
        temp.path(),
        1,
        "index_repository",
        json!({"repo_path": repo, "mode": "fast"}),
    );
    let project = successful_content(&indexed)["project"]
        .as_str()
        .expect("project")
        .to_owned();
    let inspected = inspect_file(&database, temp.path(), &project, 2);
    let locator: NodeLocator =
        serde_json::from_value(locator_with_preview(&inspected, "fn helper"))
            .expect("roundtrip locator");

    let store = Store::open(&database).expect("open edit store");
    let index = IndexService::new(
        store,
        CoreGrammarProvider,
        IndexOptions::default(),
        FileSystemDiscovery,
    );
    let journal = Store::open(&database).expect("open edit journal");
    let (mut edit, startup) = DurableEditService::open(
        index,
        journal,
        SyntaxEngine::new(CoreGrammarProvider),
        vec![temp.path().to_path_buf()],
    )
    .expect("open durable edit service");
    assert!(startup.entries.is_empty());
    edit.set_fault_injector(Arc::new(FailAfterRename));
    edit.edit_node(DurableEditRequest {
        operation_id: "stdio-recovery".to_owned(),
        locator,
        operation: EditOperation::Replace("pub fn helper() -> usize { 7 }".to_owned()),
        options: EditOptions::default(),
    })
    .expect_err("fault must leave recovery journal");
    drop(edit);

    assert!(
        fs::read_to_string(repo.join("src/lib.rs"))
            .expect("interrupted source")
            .contains("{ 7 }")
    );
    let recovered = inspect_file(&database, temp.path(), &project, 3);
    assert!(
        recovered["syntax"]["n"]
            .as_array()
            .expect("recovered syntax")
            .iter()
            .any(|node| node["v"]
                .as_str()
                .is_some_and(|preview| preview.contains("{ 7 }")))
    );
    assert!(
        Store::open(&database)
            .expect("reopen store")
            .list_incomplete_edit_operations()
            .expect("journal")
            .is_empty()
    );
}

#[test]
fn stdio_startup_reports_recovery_conflict_before_protocol_readiness() {
    let temp = TempDir::new().expect("temp directory");
    let repo = temp.path().join("fixture");
    fixture(&repo);
    let database = temp.path().join("state/graph.db");
    let indexed = call_tool(
        &database,
        temp.path(),
        1,
        "index_repository",
        json!({"repo_path": repo, "mode": "fast"}),
    );
    let project = successful_content(&indexed)["project"]
        .as_str()
        .expect("project")
        .to_owned();
    let inspected = inspect_file(&database, temp.path(), &project, 2);
    let locator: NodeLocator =
        serde_json::from_value(locator_with_preview(&inspected, "fn helper"))
            .expect("roundtrip locator");
    let store = Store::open(&database).expect("open edit store");
    let index = IndexService::new(
        store,
        CoreGrammarProvider,
        IndexOptions::default(),
        FileSystemDiscovery,
    );
    let journal = Store::open(&database).expect("open edit journal");
    let (mut edit, _) = DurableEditService::open(
        index,
        journal,
        SyntaxEngine::new(CoreGrammarProvider),
        vec![temp.path().to_path_buf()],
    )
    .expect("open durable edit service");
    edit.set_fault_injector(Arc::new(FailAfterRename));
    edit.edit_node(DurableEditRequest {
        operation_id: "stdio-recovery-conflict".to_owned(),
        locator,
        operation: EditOperation::Replace("pub fn helper() -> usize { 7 }".to_owned()),
        options: EditOptions::default(),
    })
    .expect_err("fault must leave recovery journal");
    drop(edit);
    fs::write(
        repo.join("src/lib.rs"),
        "pub fn external() -> usize { 88 }\n",
    )
    .expect("external conflicting writer");

    let output = run_server(
        b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}\n",
        &database,
        temp.path(),
    );
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "server became protocol-ready");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unresolved edit recovery conflicts"),
        "{stderr}"
    );
    assert!(stderr.contains("stdio-recovery-conflict"), "{stderr}");
    assert!(stderr.contains("src/lib.rs"), "{stderr}");
}
