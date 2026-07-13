# Goldeneye Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:subagent-driven-development (recommended), superfastpowers:goal-driven-development, or superfastpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a production-shaped Rust workspace with upstream-compatible MCP transport/lifecycle behavior and a reusable black-box contract harness.

**Architecture:** A dependency-light synchronous MCP boundary parses JSON-RPC into tool-neutral protocol types, routes requests through a truthful registry, and writes JSON only to stdout. Domain, MCP, CLI, and compatibility-test crates establish dependency direction used by later index/query/edit phases.
**Plan Acronym:** GF


**Tech Stack:** Rust 1.97.0, edition 2024, Cargo workspace resolver 3, `serde`, `serde_json`, `thiserror`, standard-library buffered I/O and process APIs.

---

## File Structure

- `Cargo.toml`: workspace members, shared package metadata, shared dependencies, lint policy.
- `rust-toolchain.toml`: reproducible Rust 1.97.0 toolchain with formatter and linter.
- `rustfmt.toml`: formatting policy.
- `crates/goldeneye-domain/src/lib.rs`: infrastructure-free shared IDs and errors.
- `crates/goldeneye-mcp/src/protocol.rs`: JSON-RPC request/response types and constructors.
- `crates/goldeneye-mcp/src/server.rs`: MCP method routing and cancellation-free synchronous request handling.
- `crates/goldeneye-mcp/src/tools.rs`: truthful tool registry, cursor pagination, tool-call envelopes.
- `crates/goldeneye-mcp/src/transport.rs`: newline JSON and `Content-Length` input framing.
- `crates/goldeneye-mcp/src/lib.rs`: public MCP boundary.
- `crates/goldeneye-cli/src/main.rs`: executable CLI and stdio server loop.
- `crates/goldeneye-cli/tests/stdio.rs`: process-level stdout/stderr and framing tests.
- `crates/goldeneye-compat-tests/src/lib.rs`: process runner and JSON normalization.
- `crates/goldeneye-compat-tests/tests/frozen_contract.rs`: frozen upstream MCP contract replay.
- `tests/fixtures/mcp/foundation.jsonl`: request fixtures.
- `tests/fixtures/mcp/foundation.expected.jsonl`: expected response fixtures.
- `NOTICE`: upstream derivative notice.
- `THIRD_PARTY.md`: dependency/license ledger seed.
- `.github/workflows/ci.yml`: format, lint, test gates.

### Task 1: Create Workspace and Domain Kernel

<TASK-ID>GF-1</TASK-ID>

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `rustfmt.toml`
- Create: `crates/goldeneye-domain/Cargo.toml`
- Create: `crates/goldeneye-domain/src/lib.rs`

- [ ] **Step 1: Create workspace manifests**

```toml
# Cargo.toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.97"
license = "MIT"
repository = "https://github.com/GoldeneyeZ/goldeneye-tool"

[workspace.dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
thiserror = "2.0"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
all = "deny"
pedantic = "deny"
```

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.97.0"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

```toml
# crates/goldeneye-domain/Cargo.toml
[package]
name = "goldeneye-domain"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write failing domain tests**

```rust
#[test]
fn project_id_rejects_empty_value() {
    assert_eq!(ProjectId::new(""), Err(DomainError::EmptyProjectId));
}

#[test]
fn project_id_preserves_valid_value() {
    let id = ProjectId::new("sample").expect("valid project ID");
    assert_eq!(id.as_str(), "sample");
}
```

- [ ] **Step 3: Run test and verify failure**

Run: `cargo test -p goldeneye-domain`

Expected: FAIL because `ProjectId` and `DomainError` are undefined.

- [ ] **Step 4: Implement domain kernel**

```rust
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DomainError {
    #[error("project ID must not be empty")]
    EmptyProjectId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectId(String);

impl ProjectId {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.is_empty() {
            return Err(DomainError::EmptyProjectId);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
```

- [ ] **Step 5: Verify workspace kernel**

Run: `cargo fmt --check && cargo clippy -p goldeneye-domain --all-targets -- -D warnings && cargo test -p goldeneye-domain`

Expected: all commands exit 0; two tests pass.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml rust-toolchain.toml rustfmt.toml crates/goldeneye-domain
git commit -m "build: create Goldeneye Rust workspace"
```

### Task 2: Define JSON-RPC Protocol Types

<TASK-ID>GF-2</TASK-ID>

**Files:**
- Create: `crates/goldeneye-mcp/Cargo.toml`
- Create: `crates/goldeneye-mcp/src/lib.rs`
- Create: `crates/goldeneye-mcp/src/protocol.rs`

- [ ] **Step 1: Create MCP crate manifest**

```toml
[package]
name = "goldeneye-mcp"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write failing protocol tests**

```rust
#[test]
fn request_accepts_numeric_and_string_ids() {
    let numeric = Request::parse(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#)
        .expect("numeric ID");
    let string = Request::parse(r#"{"jsonrpc":"2.0","id":"abc","method":"ping"}"#)
        .expect("string ID");
    assert_eq!(numeric.id, Some(RequestId::Number(7)));
    assert_eq!(string.id, Some(RequestId::String("abc".into())));
}

#[test]
fn missing_id_is_notification() {
    let request = Request::parse(r#"{"jsonrpc":"2.0","method":"notifications/cancelled"}"#)
        .expect("notification");
    assert!(request.is_notification());
}
```

- [ ] **Step 3: Run test and verify failure**

Run: `cargo test -p goldeneye-mcp protocol`

Expected: FAIL because protocol types are undefined.

- [ ] **Step 4: Implement protocol module**

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    String(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    #[serde(default)]
    pub id: Option<RequestId>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn parse(input: &str) -> serde_json::Result<Self> {
        serde_json::from_str(input)
    }

    #[must_use]
    pub const fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Serialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Option<RequestId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

impl Response {
    pub fn success(id: RequestId, result: Value) -> Self {
        Self { jsonrpc: "2.0", id: Some(id), result: Some(result), error: None }
    }

    pub fn error(id: Option<RequestId>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(ErrorObject { code, message: message.into() }),
        }
    }
}
```

- [ ] **Step 5: Verify protocol types**

Run: `cargo fmt --check && cargo clippy -p goldeneye-mcp --all-targets -- -D warnings && cargo test -p goldeneye-mcp protocol`

Expected: all commands exit 0; protocol tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/goldeneye-mcp
git commit -m "feat: define MCP JSON-RPC protocol"
```

### Task 3: Implement MCP Lifecycle Routing

<TASK-ID>GF-3</TASK-ID>

**Files:**
- Create: `crates/goldeneye-mcp/src/server.rs`
- Modify: `crates/goldeneye-mcp/src/lib.rs`
- Test: `crates/goldeneye-mcp/src/server.rs`

- [ ] **Step 1: Write failing lifecycle tests**

```rust
#[test]
fn initialize_returns_upstream_identity_and_latest_protocol() {
    let response = Server::default().handle_line(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
    ).expect("request response");
    let value = serde_json::to_value(response).expect("serialize response");
    assert_eq!(value["result"]["protocolVersion"], "2025-11-25");
    assert_eq!(value["result"]["serverInfo"]["name"], "codebase-memory-mcp");
    assert_eq!(value["result"]["capabilities"]["tools"]["listChanged"], false);
}

#[test]
fn invalid_json_and_unknown_method_use_jsonrpc_errors() {
    let server = Server::default();
    let parse = server.handle_line("{").expect("parse error response");
    let unknown = server.handle_line(
        r#"{"jsonrpc":"2.0","id":"x","method":"missing"}"#,
    ).expect("method error response");
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
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p goldeneye-mcp server`

Expected: FAIL because `Server` is undefined.

- [ ] **Step 3: Implement lifecycle server**

```rust
use crate::protocol::{Request, Response};
use serde_json::{json, Value};

pub const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";

#[derive(Default)]
pub struct Server;

impl Server {
    #[must_use]
    pub fn handle_line(&self, line: &str) -> Option<Response> {
        let request = match Request::parse(line) {
            Ok(request) => request,
            Err(error) => return Some(Response::error(None, -32700, error.to_string())),
        };
        let Some(id) = request.id.clone() else {
            return None;
        };
        let result: Option<Value> = match request.method.as_str() {
            "initialize" => Some(json!({
                "protocolVersion": LATEST_PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "codebase-memory-mcp", "version": env!("CARGO_PKG_VERSION") }
            })),
            "ping" => Some(json!({})),
            "resources/list" => Some(json!({ "resources": [] })),
            "resources/templates/list" => Some(json!({ "resourceTemplates": [] })),
            "prompts/list" => Some(json!({ "prompts": [] })),
            _ => None,
        };
        Some(match result {
            Some(value) => Response::success(id, value),
            None => Response::error(Some(id), -32601, "Method not found"),
        })
    }
}
```

- [ ] **Step 4: Verify lifecycle behavior**

Run: `cargo test -p goldeneye-mcp server`

Expected: lifecycle tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/goldeneye-mcp
git commit -m "feat: implement MCP lifecycle routing"
```

### Task 4: Add Truthful Tool Registry and Pagination

<TASK-ID>GF-4</TASK-ID>

**Files:**
- Create: `crates/goldeneye-mcp/src/tools.rs`
- Modify: `crates/goldeneye-mcp/src/server.rs`
- Modify: `crates/goldeneye-mcp/src/lib.rs`
- Test: `crates/goldeneye-mcp/src/tools.rs`

- [ ] **Step 1: Write failing pagination and unknown-tool tests**

```rust
#[test]
fn registry_returns_all_without_cursor_and_pages_when_cursor_present() {
    let tools = (0..10).map(|index| ToolDefinition::test(format!("tool-{index}"))).collect();
    let registry = ToolRegistry::new(tools);
    let all = registry.page(None).expect("unpaginated list");
    assert_eq!(all.tools.len(), 10);
    assert!(all.next_cursor.is_none());
    let first = registry.page(Some("0")).expect("first page");
    assert_eq!(first.tools.len(), 8);
    assert_eq!(first.next_cursor.as_deref(), Some("8"));
    let second = registry.page(first.next_cursor.as_deref()).expect("second page");
    assert_eq!(second.tools.len(), 2);
    assert!(second.next_cursor.is_none());
}

#[test]
fn empty_registry_advertises_no_tools() {
    let page = ToolRegistry::default().page(None).expect("empty page");
    assert!(page.tools.is_empty());
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p goldeneye-mcp tools`

Expected: FAIL because registry types are undefined.

- [ ] **Step 3: Implement registry and MCP result envelope**

```rust
use serde::Serialize;
use serde_json::{json, Value};

const PAGE_SIZE: usize = 8;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDefinition {
    pub name: String,
    pub title: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
}

impl ToolDefinition {
    #[cfg(test)]
    pub fn test(name: String) -> Self {
        Self {
            title: name.clone(),
            description: name.clone(),
            name,
            input_schema: json!({"type":"object"}),
            output_schema: json!({"type":"object", "additionalProperties": true}),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPage {
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

#[derive(Default)]
pub struct ToolRegistry {
    tools: Vec<ToolDefinition>,
}

impl ToolRegistry {
    #[must_use]
    pub const fn new(tools: Vec<ToolDefinition>) -> Self { Self { tools } }

    pub fn page(&self, cursor: Option<&str>) -> Result<ToolPage, &'static str> {
        let Some(cursor) = cursor else {
            return Ok(ToolPage { tools: self.tools.clone(), next_cursor: None });
        };
        let offset = cursor.parse::<usize>().map_err(|_| "invalid cursor")?;
        if offset > self.tools.len() { return Err("invalid cursor"); }
        let end = (offset + PAGE_SIZE).min(self.tools.len());
        Ok(ToolPage {
            tools: self.tools[offset..end].to_vec(),
            next_cursor: (end < self.tools.len()).then(|| end.to_string()),
        })
    }
}
```

Update `Server` to own `ToolRegistry`, route `tools/list` through `page`, and return unknown `tools/call` names as an MCP tool result:

```json
{"content":[{"type":"text","text":"Unknown tool: missing"}],"isError":true}
```

- [ ] **Step 4: Verify registry and routing**

Run: `cargo test -p goldeneye-mcp`

Expected: all MCP unit tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/goldeneye-mcp
git commit -m "feat: add truthful MCP tool registry"
```

### Task 5: Implement Newline and Content-Length Framing

<TASK-ID>GF-5</TASK-ID>

**Files:**
- Create: `crates/goldeneye-mcp/src/transport.rs`
- Modify: `crates/goldeneye-mcp/src/lib.rs`
- Test: `crates/goldeneye-mcp/src/transport.rs`

- [ ] **Step 1: Write failing framing tests**

```rust
#[test]
fn reads_newline_delimited_json() {
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
    let mut reader = FrameReader::new(&input[..]);
    assert_eq!(reader.next_frame().expect("read").expect("frame"), &input[..input.len() - 1]);
}

#[test]
fn reads_content_length_frame() {
    let body = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}";
    let input = format!("Content-Length: {}\r\n\r\n{}", body.len(), String::from_utf8_lossy(body));
    let mut reader = FrameReader::new(input.as_bytes());
    assert_eq!(reader.next_frame().expect("read").expect("frame"), body);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p goldeneye-mcp transport`

Expected: FAIL because `FrameReader` is undefined.

- [ ] **Step 3: Implement bounded frame reader**

Implement `FrameReader<R: BufRead>` with:

```rust
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FrameError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid Content-Length header")]
    InvalidHeader,
    #[error("frame contains {size} bytes; limit is {limit}")]
    FrameTooLarge { size: usize, limit: usize },
    #[error("frame ended before declared Content-Length")]
    UnexpectedEof,
}

impl<R: BufRead> FrameReader<R> {
    pub fn new(reader: R) -> Self;
    pub fn next_frame(&mut self) -> Result<Option<Vec<u8>>, FrameError>;
}
```

Algorithm:

1. Read first line with `read_until(b'\n', ...)`.
2. Return `None` on clean EOF.
3. If line begins case-insensitively with `Content-Length:`, parse bounded length, consume headers through blank line, then `read_exact` body.
4. Otherwise trim one trailing LF and optional CR and return line as JSON frame.
5. Reject any declared or accumulated frame over `MAX_FRAME_BYTES`.

- [ ] **Step 4: Verify framing**

Run: `cargo test -p goldeneye-mcp transport`

Expected: newline, CRLF, `Content-Length`, clean EOF, malformed header, oversized frame, and truncated body tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/goldeneye-mcp
git commit -m "feat: support MCP stdio framing"
```

### Task 6: Build CLI Stdio Server

<TASK-ID>GF-6</TASK-ID>

**Files:**
- Create: `crates/goldeneye-cli/Cargo.toml`
- Create: `crates/goldeneye-cli/src/lib.rs`
- Create: `crates/goldeneye-cli/src/main.rs`
- Create: `crates/goldeneye-cli/tests/stdio.rs`

- [ ] **Step 1: Create CLI manifest and failing process test**

```toml
[package]
name = "goldeneye"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
goldeneye-mcp = { path = "../goldeneye-mcp" }
serde_json.workspace = true

[lints]
workspace = true
```

```rust
#[test]
fn ping_round_trip_keeps_stdout_json_only() {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_goldeneye"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn goldeneye");
    use std::io::Write;
    child.stdin.take().expect("stdin").write_all(
        b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n",
    ).expect("write request");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON-only stdout");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"], serde_json::json!({}));
}
```

- [ ] **Step 2: Run test and verify failure**

Run: `cargo test -p goldeneye --test stdio`

Expected: FAIL because binary server loop is absent.

- [ ] **Step 3: Implement CLI and stdio loop**

```rust
// crates/goldeneye-cli/src/lib.rs
use goldeneye_mcp::{FrameReader, Server};
use std::io::{BufReader, BufWriter, Read, Write};

pub fn run_session<R: Read, W: Write>(reader: R, writer: W) -> Result<(), Box<dyn std::error::Error>> {
    let mut frames = FrameReader::new(BufReader::new(reader));
    let mut output = BufWriter::new(writer);
    let server = Server::default();
    while let Some(frame) = frames.next_frame()? {
        let line = String::from_utf8(frame)?;
        if let Some(response) = server.handle_line(&line) {
            serde_json::to_writer(&mut output, &response)?;
            output.write_all(b"\n")?;
            output.flush()?;
        }
    }
    Ok(())
}
```

```rust
// crates/goldeneye-cli/src/main.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|arg| arg == "--version") {
        println!("goldeneye {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    goldeneye::run_session(std::io::stdin().lock(), std::io::stdout().lock())
}
```

- [ ] **Step 4: Verify process behavior**

Run: `cargo test -p goldeneye --test stdio`

Expected: ping, initialize, invalid JSON, string-ID, notification, and `Content-Length` process tests pass; stdout parses as JSON lines.

- [ ] **Step 5: Commit**

```bash
git add crates/goldeneye-cli
git commit -m "feat: run Goldeneye MCP over stdio"
```

### Task 7: Add Frozen Compatibility Harness, Notices, and CI

<TASK-ID>GF-7</TASK-ID>

**Files:**
- Create: `crates/goldeneye-compat-tests/Cargo.toml`
- Create: `crates/goldeneye-compat-tests/src/lib.rs`
- Create: `crates/goldeneye-compat-tests/tests/frozen_contract.rs`
- Create: `tests/fixtures/mcp/foundation.jsonl`
- Create: `tests/fixtures/mcp/foundation.expected.jsonl`
- Create: `NOTICE`
- Create: `THIRD_PARTY.md`
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Create compatibility crate manifest**

```toml
[package]
name = "goldeneye-compat-tests"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
goldeneye = { path = "../goldeneye-cli" }
serde_json.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write frozen contract fixture**

`tests/fixtures/mcp/foundation.jsonl` contains initialize, ping, resource/template/prompt probes, tools list, invalid method, string request ID, notification, and invalid JSON requests. `foundation.expected.jsonl` contains one normalized expected response per non-notification request, using upstream identity and error codes.

- [ ] **Step 3: Write failing replay test**

```rust
#[test]
fn goldeneye_matches_frozen_foundation_contract() {
    let root = workspace_root();
    let actual = run_jsonl(&root.join("tests/fixtures/mcp/foundation.jsonl"))
        .expect("run Goldeneye");
    let expected = read_jsonl(&root.join("tests/fixtures/mcp/foundation.expected.jsonl"))
        .expect("read expected responses");
    assert_eq!(normalize(actual), normalize(expected));
}
```

- [ ] **Step 4: Run replay test and verify failure**

Run: `cargo test -p goldeneye-compat-tests --test frozen_contract`

Expected: FAIL until runner, normalization, binary discovery, and fixtures are wired.

- [ ] **Step 5: Implement compatibility utilities**

Implement:

```rust
pub fn workspace_root() -> PathBuf;
pub fn run_jsonl(requests: &Path) -> io::Result<Vec<Value>>;
pub fn read_jsonl(path: &Path) -> io::Result<Vec<Value>>;
pub fn normalize(values: Vec<Value>) -> Vec<Value>;
```

`run_jsonl` reads fixture bytes, invokes `goldeneye::run_session` with in-memory input/output, and parses every nonempty output line as JSON. Process-level stdout purity remains covered by Task 6.

Normalization may remove only nondeterministic version/build fields documented in the test. It must preserve IDs, protocol version, method results, error codes/messages, pagination fields, tool schemas, and response order.

- [ ] **Step 6: Add legal notices**

`NOTICE` must state Goldeneye derives from `codebase-memory-mcp`, copyright `(c) 2025 DeusData`, under MIT, and identify audited commit. `THIRD_PARTY.md` starts a ledger for upstream MIT code, Tree-sitter runtime/grammars, and Rust crates; grammar-specific notices expand when grammar assets enter production.

- [ ] **Step 7: Add CI gates**

```yaml
name: ci
on: [push, pull_request]
jobs:
  rust:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.97.0
        with:
          components: rustfmt, clippy
      - run: cargo fmt --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
```

- [ ] **Step 8: Verify complete foundation slice**

Run: `cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

Expected: all workspace, process, framing, and frozen-contract tests pass on local platform.

- [ ] **Step 9: Commit**

```bash
git add crates/goldeneye-compat-tests tests/fixtures NOTICE THIRD_PARTY.md .github/workflows/ci.yml
git commit -m "test: freeze Goldeneye MCP foundation contract"
```

+### Task 8: Repair Foundation Integration Compatibility

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

- [ ] **Step 1: Write failing protocol negotiation tests**

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

- [ ] **Step 2: Write failing upstream parse-compatibility tests**

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

- [ ] **Step 3: Write failing invalid-UTF-8 continuity process test**

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

- [ ] **Step 4: Run focused tests and verify RED**

Run:

```bash
cargo test -p goldeneye-mcp
cargo test -p goldeneye --test stdio invalid_utf8_returns_parse_error_then_processes_next_frame
```

Expected: failures show unsupported version echo, missing `jsonrpc` rejection, unstable parse error, and UTF-8 session termination.

- [ ] **Step 5: Implement exact upstream protocol behavior**

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

- [ ] **Step 6: Freeze corrected upstream cases**

Extend request/expected fixtures with:

- initialize requests for all four supported versions and one unsupported version;
- missing-`jsonrpc` ping success;
- malformed object and top-level array parse errors;
- parse errors fixed to `{"jsonrpc":"2.0","id":0,"error":{"code":-32700,"message":"Parse error"}}`.

Invalid UTF-8 remains a binary process fixture in stdio tests because JSONL cannot represent byte `0xff`.

- [ ] **Step 7: Verify GREEN and full foundation gate**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: all commands exit 0; focused negotiation, parse, UTF-8 continuity, process, and frozen tests pass.

- [ ] **Step 8: Update final review and commit**

Replace prior final-review verdict with a repair record mapping each Important finding to exact tests and results. Commit implementation and task evidence:

```bash
git add crates tests docs/superfastpowers/plans/GF
git commit -m "[GF-8] fix: align MCP foundation compatibility"
```
