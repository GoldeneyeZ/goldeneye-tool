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
