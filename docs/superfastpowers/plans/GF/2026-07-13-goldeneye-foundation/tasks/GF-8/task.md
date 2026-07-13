### Task 8: Repair Foundation Integration Compatibility

<TASK-ID>GF-8</TASK-ID>

**Files:**
- Modify: `crates/goldeneye-mcp/src/protocol.rs`
- Modify: `crates/goldeneye-mcp/src/server.rs`
- Modify: `crates/goldeneye-cli/src/lib.rs`
- Modify: `crates/goldeneye-cli/tests/stdio.rs`
- Modify: `crates/goldeneye-compat-tests/src/lib.rs`
- Modify: `crates/goldeneye-compat-tests/tests/frozen_contract.rs`
- Modify: `tests/fixtures/mcp/foundation.jsonl`
- Modify: `tests/fixtures/mcp/foundation.expected.jsonl`
- Modify: `docs/superfastpowers/plans/GF/2026-07-13-goldeneye-foundation/final-review.md`

- [x] **Step 1: Write failing protocol negotiation tests**

```rust
#[test]
fn initialize_echoes_every_supported_protocol_version() {
    for version in ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"] {
        let request = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{version}"}}}}"#
        );
        let response = Server::default().handle_line(&request).expect("response");
        assert_eq!(response.result.expect("result")["protocolVersion"], version);
    }
}

#[test]
fn initialize_falls_back_to_latest_for_unsupported_version() {
    let response = Server::default().handle_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"unsupported"}}"#,
    ).expect("response");
    assert_eq!(response.result.expect("result")["protocolVersion"], "2025-11-25");
}
```

- [x] **Step 2: Write failing upstream parse-compatibility tests**

```rust
#[test]
fn request_defaults_missing_jsonrpc_to_2_0() {
    let request = Request::parse(r#"{"id":7,"method":"ping"}"#).expect("upstream-compatible request");
    assert_eq!(request.jsonrpc, "2.0");
}

#[test]
fn parse_failures_use_stable_upstream_error() {
    for input in ["{", "[]", r#"{"jsonrpc":"2.0","id":1}"#] {
        let response = Server::default().handle_line(input).expect("parse response");
        let error = response.error.expect("error");
        assert_eq!(response.id, Some(RequestId::Number(0)));
        assert_eq!(error.code, -32700);
        assert_eq!(error.message, "Parse error");
    }
}
```

- [x] **Step 3: Write failing invalid-UTF-8 continuity process test**

```rust
#[test]
fn invalid_utf8_returns_parse_error_then_processes_next_frame() {
    let input = [
        &[0xff, b'\n'][..],
        b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n",
    ].concat();
    let output = run_process(&input);
    assert!(output.status.success());
    let responses = parse_json_lines(&output.stdout);
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[0]["id"], 0);
    assert_eq!(responses[0]["error"]["code"], -32700);
    assert_eq!(responses[0]["error"]["message"], "Parse error");
    assert_eq!(responses[1]["id"], 7);
    assert_eq!(responses[1]["result"], serde_json::json!({}));
}
```

- [x] **Step 4: Run focused tests and verify RED**

Run:

```bash
cargo test -p goldeneye-mcp
cargo test -p goldeneye --test stdio invalid_utf8_returns_parse_error_then_processes_next_frame
```

Expected: failures show unsupported version echo, missing `jsonrpc` rejection, unstable parse error, and UTF-8 session termination.

- [x] **Step 5: Implement exact upstream protocol behavior**

In `protocol.rs`, default missing `jsonrpc` and expose one stable parse-error constructor:

```rust
fn default_jsonrpc() -> String {
    "2.0".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[serde(default = "default_jsonrpc")]
    pub jsonrpc: String,
    // existing fields
}

impl Response {
    #[must_use]
    pub fn parse_error() -> Self {
        Self::error(Some(RequestId::Number(0)), -32700, "Parse error")
    }
}
```

In `server.rs`, define audited versions newest-first and negotiate from `params.protocolVersion` only when string and supported:

```rust
pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 4] =
    ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

fn negotiated_protocol_version(params: &serde_json::Value) -> &'static str {
    params
        .get("protocolVersion")
        .and_then(serde_json::Value::as_str)
        .and_then(|requested| {
            SUPPORTED_PROTOCOL_VERSIONS
                .iter()
                .copied()
                .find(|supported| requested == *supported)
        })
        .unwrap_or(SUPPORTED_PROTOCOL_VERSIONS[0])
}
```

Map every request parse failure to `Response::parse_error()`, never raw Serde wording.

In `goldeneye-cli/src/lib.rs`, keep session alive on invalid UTF-8:

```rust
let response = match String::from_utf8(frame) {
    Ok(line) => server.handle_line(&line),
    Err(_) => Some(goldeneye_mcp::Response::parse_error()),
};
if let Some(response) = response {
    serde_json::to_writer(&mut output, &response)?;
    output.write_all(b"\n")?;
    output.flush()?;
}
```

- [x] **Step 6: Freeze corrected upstream cases**

Extend request/expected fixtures with:

- initialize requests for all four supported versions and one unsupported version;
- missing-`jsonrpc` ping success;
- malformed object and top-level array parse errors;
- parse errors fixed to `{"jsonrpc":"2.0","id":0,"error":{"code":-32700,"message":"Parse error"}}`.

Invalid UTF-8 remains a binary process fixture in stdio tests because JSONL cannot represent byte `0xff`.

- [x] **Step 7: Verify GREEN and full foundation gate**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: all commands exit 0; focused negotiation, parse, UTF-8 continuity, process, and frozen tests pass.

- [x] **Step 8: Update final review and commit**

Replace prior final-review verdict with a repair record mapping each Important finding to exact tests and results. Commit implementation and task evidence:

```bash
git add crates tests docs/superfastpowers/plans/GF
git commit -m "[GF-8] fix: align MCP foundation compatibility"
```
