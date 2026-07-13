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
