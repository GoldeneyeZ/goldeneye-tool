use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Output, Stdio};

use goldeneye_mcp::server::Server;
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
    ] {
        second_input.push_str(&value.to_string());
        second_input.push('\n');
    }
    let second = responses(&run_server(second_input.as_bytes(), &database, temp.path()));
    assert_eq!(second.len(), 4);
    assert_eq!(
        second[0]["result"]["tools"]
            .as_array()
            .expect("tools")
            .len(),
        10
    );
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
}
