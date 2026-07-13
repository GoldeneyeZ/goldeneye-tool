### Task 6: Repair Generated Language TSV Whitespace

<TASK-ID>GD-6</TASK-ID>

**Files:**
- Modify: `tools/export_upstream_languages.py`
- Modify: `crates/goldeneye-discovery/data/languages.tsv`
- Modify: `crates/goldeneye-discovery/src/language.rs`
- Modify: `crates/goldeneye-discovery/tests/language_parity.rs`

- [ ] **Step 1: Add failing generated-data hygiene test**

```rust
#[test]
fn generated_language_data_has_no_trailing_whitespace() {
    for (index, line) in include_str!("../data/languages.tsv").lines().enumerate() {
        assert_eq!(line.trim_end(), line, "trailing whitespace on TSV line {}", index + 1);
    }
}
```

- [ ] **Step 2: Verify RED**

Run: `cargo test -p goldeneye-discovery --test language_parity generated_language_data_has_no_trailing_whitespace`

Expected: FAIL on first language row whose empty final columns are encoded as trailing tabs.

- [ ] **Step 3: Fix root cause at exporter/parser boundary**

Exporter writes `-` for each empty list field instead of an empty final TSV cell. Registry parser treats an exact `-` field as an empty list and rejects mixed sentinel/data fields.

Regenerate:

```bash
python tools/export_upstream_languages.py --upstream .upstream/codebase-memory-mcp --output crates/goldeneye-discovery/data/languages.tsv
```

- [ ] **Step 4: Verify exact data parity and clean phase diff**

Run:

```bash
cargo test -p goldeneye-discovery --test language_parity
cargo test --workspace
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
git diff --check 13d741d..HEAD
```

Expected: registry remains `160/239/33/1`; exporter reproduction passes; every command exits 0.

- [ ] **Step 5: Commit**

```bash
git add tools/export_upstream_languages.py crates/goldeneye-discovery docs/superfastpowers/plans/GD
git commit -m "[GD-6] fix: remove generated TSV trailing whitespace"
```
