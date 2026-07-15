use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use goldeneye_services::ProjectId;
use goldeneye_store::Store;
use serde_json::{Value, json};
use tempfile::TempDir;

fn fixture(root: &Path) {
    fs::create_dir_all(root.join("src")).expect("source directory");
    fs::write(root.join("src/lib.rs"), "pub fn entry() -> usize { 1 }\n").expect("source file");
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

fn response(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "server failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "stderr must remain empty: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("UTF-8 stdout");
    let mut lines = stdout.lines();
    let value = serde_json::from_str(lines.next().expect("one response")).expect("JSON response");
    assert!(lines.next().is_none(), "exactly one response: {stdout}");
    value
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
    response(&run_server(input.as_bytes(), database, root))["result"].clone()
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
#[allow(clippy::too_many_lines)]
fn stdio_adr_and_trace_tools_survive_clean_restarts() {
    let temp = TempDir::new().expect("temp dir");
    let root = temp.path().join("project");
    let database = temp.path().join("graph.sqlite3");
    fixture(&root);

    let indexed = call_tool(
        &database,
        &root,
        1,
        "index_repository",
        json!({"repo_path": "."}),
    );
    let project = successful(&indexed)["project"]
        .as_str()
        .expect("project")
        .to_owned();

    fs::create_dir(root.join(".codebase-memory")).expect("legacy directory");
    fs::write(
        root.join(".codebase-memory/adr.md"),
        "# Legacy\n## PURPOSE\nImported over stdio",
    )
    .expect("legacy ADR");
    assert_eq!(
        successful(&call_tool(
            &database,
            &root,
            2,
            "manage_adr",
            json!({"project": project, "mode": "get"}),
        )),
        &json!({"content": "# Legacy\n## PURPOSE\nImported over stdio"})
    );

    assert_eq!(
        successful(&call_tool(
            &database,
            &root,
            3,
            "manage_adr",
            json!({
                "project": project,
                "mode": "update",
                "content": "# Durable\n## PURPOSE\nRestart-safe\n## STACK\nRust"
            }),
        )),
        &json!({"status": "updated"})
    );
    assert_eq!(
        successful(&call_tool(
            &database,
            &root,
            4,
            "manage_adr",
            json!({"project": project}),
        )),
        &json!({"content": "# Durable\n## PURPOSE\nRestart-safe\n## STACK\nRust"})
    );
    assert_eq!(
        successful(&call_tool(
            &database,
            &root,
            5,
            "manage_adr",
            json!({"project": project, "mode": "sections"}),
        )),
        &json!({"sections": ["# Durable", "## PURPOSE", "## STACK"]})
    );

    assert_eq!(
        successful(&call_tool(
            &database,
            &root,
            6,
            "ingest_traces",
            json!({
                "project": project,
                "traces": [
                    {"caller": "entry", "callee": "helper"},
                    {"caller": "entry", "callee": "helper", "count": 4},
                    {"caller": "partial"}
                ]
            }),
        )),
        &json!({
            "status": "accepted",
            "traces_received": 3,
            "note": "Runtime edge creation from traces not yet implemented"
        })
    );

    let project_id = ProjectId::new(project).expect("project ID");
    let store = Store::open_read_only(&database).expect("reopen database");
    assert_eq!(
        store
            .get_adr(&project_id)
            .expect("ADR")
            .expect("stored ADR")
            .content,
        "# Durable\n## PURPOSE\nRestart-safe\n## STACK\nRust"
    );
    let traces = store
        .list_runtime_traces(&project_id)
        .expect("runtime traces");
    assert_eq!(traces.len(), 1);
    assert_eq!(traces[0].count, 5);
}
