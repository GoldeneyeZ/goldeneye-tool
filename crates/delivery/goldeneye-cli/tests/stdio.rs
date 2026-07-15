use serde_json::{Value, json};
use std::io::{Cursor, Write};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;

use goldeneye_bootstrap::BootstrapRuntime;
use goldeneye_services::ServiceConfig;

fn run_server(input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_goldeneye"))
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
        .expect("write request");

    child.wait_with_output().expect("wait for goldeneye")
}

fn parse_json_lines(stdout: &[u8]) -> Vec<Value> {
    let text = std::str::from_utf8(stdout).expect("UTF-8 stdout");
    text.lines()
        .map(|line| serde_json::from_str(line).expect("JSON-only stdout line"))
        .collect()
}

fn assert_clean_success(output: &Output) {
    assert!(
        output.status.success(),
        "goldeneye failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn injected_runtime_stops_on_stdio_eof() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let runtime = BootstrapRuntime::from_config(ServiceConfig::new(
        temp.path().join("graph.db"),
        temp.path(),
    ));
    let watcher = Arc::clone(runtime.watcher());
    let mut output = Vec::new();

    goldeneye::run_session_with_runtime(Cursor::new(Vec::<u8>::new()), &mut output, runtime)
        .expect("EOF session");

    assert!(output.is_empty());
    assert!(watcher.is_stopped());
}

#[test]
fn newline_ping_round_trip_preserves_numeric_id_and_json_stdout() {
    let output = run_server(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n");
    assert_clean_success(&output);

    let responses = parse_json_lines(&output.stdout);
    assert_eq!(
        responses,
        vec![json!({"jsonrpc": "2.0", "id": 1, "result": {}})]
    );
}

#[test]
fn initialize_round_trip_returns_protocol_and_server_identity() {
    let output =
        run_server(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"initialize\",\"params\":{}}\n");
    assert_clean_success(&output);

    let responses = parse_json_lines(&output.stdout);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["id"], 2);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(
        responses[0]["result"]["serverInfo"]["name"],
        "codebase-memory-mcp"
    );
}

#[test]
fn initialize_round_trip_negotiates_supported_versions_and_falls_back() {
    for (requested, expected) in [
        ("2025-11-25", "2025-11-25"),
        ("2025-06-18", "2025-06-18"),
        ("2025-03-26", "2025-03-26"),
        ("2024-11-05", "2024-11-05"),
        ("unsupported", "2025-11-25"),
    ] {
        let input = format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"initialize","params":{{"protocolVersion":"{requested}"}}}}
"#
        );
        let output = run_server(input.as_bytes());
        assert_clean_success(&output);

        let responses = parse_json_lines(&output.stdout);
        assert_eq!(
            responses[0]["result"]["protocolVersion"], expected,
            "requested {requested}"
        );
    }
}

#[test]
fn ping_round_trip_preserves_string_id() {
    let output = run_server(b"{\"jsonrpc\":\"2.0\",\"id\":\"request-1\",\"method\":\"ping\"}\n");
    assert_clean_success(&output);

    let responses = parse_json_lines(&output.stdout);
    assert_eq!(
        responses,
        vec![json!({"jsonrpc": "2.0", "id": "request-1", "result": {}})]
    );
}

#[test]
fn notification_produces_no_stdout() {
    let output = run_server(
        b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/cancelled\",\"params\":{\"requestId\":1}}\n",
    );
    assert_clean_success(&output);

    assert!(output.stdout.is_empty());
}

#[test]
fn invalid_json_returns_parse_error_as_json() {
    let output = run_server(b"{\n");
    assert_clean_success(&output);

    let responses = parse_json_lines(&output.stdout);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["jsonrpc"], "2.0");
    assert_eq!(responses[0]["id"], 0);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[0]["error"]["message"], "Parse error");
}

#[test]
fn invalid_utf8_returns_parse_error_then_processes_next_frame() {
    let input = [
        &[0xff, b'\n'][..],
        b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n",
    ]
    .concat();

    let output = run_server(&input);
    assert_clean_success(&output);

    let responses = parse_json_lines(&output.stdout);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], 0);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[0]["error"]["message"], "Parse error");
    assert_eq!(responses[1]["id"], 7);
    assert_eq!(responses[1]["result"], json!({}));
}

#[test]
fn content_length_ping_round_trip_uses_same_json_stdout() {
    let body = b"{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"ping\"}";
    let mut input = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    input.extend_from_slice(body);

    let output = run_server(&input);
    assert_clean_success(&output);

    let responses = parse_json_lines(&output.stdout);
    assert_eq!(
        responses,
        vec![json!({"jsonrpc": "2.0", "id": 3, "result": {}})]
    );
}

#[test]
fn version_flag_prints_package_version_and_exits() {
    let output = Command::new(env!("CARGO_BIN_EXE_goldeneye"))
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run goldeneye --version");

    assert_clean_success(&output);
    assert_eq!(
        std::str::from_utf8(&output.stdout).expect("UTF-8 version output"),
        concat!("goldeneye ", env!("CARGO_PKG_VERSION"), "\n")
    );
}
