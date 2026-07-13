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
fn registry_pages_eight_tools_and_emits_cursor() {
    let tools = (0..10).map(|index| ToolDefinition::test(format!("tool-{index}"))).collect();
    let registry = ToolRegistry::new(tools);
    let first = registry.page(None).expect("first page");
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
}

impl ToolDefinition {
    #[cfg(test)]
    pub fn test(name: String) -> Self {
        Self { title: name.clone(), description: name.clone(), name, input_schema: json!({"type":"object"}) }
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
        let offset = cursor.unwrap_or("0").parse::<usize>().map_err(|_| "invalid cursor")?;
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
