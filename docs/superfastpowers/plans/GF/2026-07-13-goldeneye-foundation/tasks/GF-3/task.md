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
