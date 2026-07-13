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
