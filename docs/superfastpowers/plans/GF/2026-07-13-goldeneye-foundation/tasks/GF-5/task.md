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
