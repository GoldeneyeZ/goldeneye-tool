### Task 5: Freeze Upstream Discovery Parity and Legal Evidence

<TASK-ID>GD-5</TASK-ID>

**Files:**
- Create: `crates/goldeneye-discovery/tests/upstream_parity.rs`
- Create: `crates/goldeneye-discovery/tests/fixtures/discovery/manifest.tsv`
- Modify: `THIRD_PARTY.md`
- Modify: `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/plan-progression.md`

- [ ] **Step 1: Build frozen parity fixture**

Create one fixture repository in test setup covering:

- root and nested `.gitignore`;
- global ignore supplied explicitly;
- root and nested `.cbmignore` negation;
- always-skip and fast-skip directories;
- always and fast suffixes;
- fast filenames/patterns;
- supported, unsupported, exact-name, compound, hidden, Unicode, and spaced paths;
- symlink;
- oversized file.

`manifest.tsv` records each path, mode, expected disposition, language ID, and ignore reason. Each row cites matching upstream test or source policy line in a comment column.

- [ ] **Step 2: Write failing fixture replay**

```rust
#[test]
fn full_moderate_and_fast_reports_match_frozen_upstream_manifest() {
    let fixture = UpstreamFixture::materialize();
    for mode in [IndexMode::Full, IndexMode::Moderate, IndexMode::Fast] {
        let actual = discover(fixture.root(), &fixture.options(mode)).unwrap();
        assert_eq!(normalize_report(actual), fixture.expected(mode));
    }
}
```

- [ ] **Step 3: Run replay and verify RED**

Run: `cargo test -p goldeneye-discovery --test upstream_parity`

Expected: FAIL until all manifest rows and normalization are wired.

- [ ] **Step 4: Implement fixture helpers and repair only proven differences**

Normalization may convert path separators to `/` and omit platform-specific permission warnings. It must preserve file membership, language, exclusion reason, mode, ordering, size, and ignored totals.

- [ ] **Step 5: Update legal ledger**

Add `ignore 0.4.28` and transitive crates with licenses/source links. Record language TSV as MIT-derived data from audited upstream commit and preserve upstream notice.

- [ ] **Step 6: Run complete discovery gate**

Run:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: all commands exit 0; frozen discovery parity passes for all modes.

- [ ] **Step 7: Commit**

```bash
git add crates/goldeneye-discovery tools/export_upstream_languages.py THIRD_PARTY.md docs/superfastpowers/plans/GD
git commit -m "[GD-5] test: freeze repository discovery parity"
```
