### Task 5: Freeze Full Grammar-Pack Metadata and Offline Sync

<TASK-ID>GS-5</TASK-ID>

This is an intermediate metadata/materialization slice. It does **not** claim 160-language runtime completion: release compilation, generated `FullGrammarProvider`, every-grammar parse probes, the full-pack CI job, and release embedding belong to the required successor phase **GFP — Full Grammar Provider Runtime**. Until GFP passes, only the six core grammars are executable and release builds are not full-pack completion evidence.

**Files:**
- Create: `grammars/full-pack.toml`
- Create: `tools/export_grammar_lock.py`
- Create: `.cargo/config.toml`
- Create: `xtask/Cargo.toml`
- Create: `xtask/src/main.rs`
- Create: `xtask/tests/grammar_sync.rs`
- Create: `crates/goldeneye-syntax/src/pack.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Modify: `crates/goldeneye-syntax/Cargo.toml`
- Create: `crates/goldeneye-syntax/tests/grammar_lock.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `THIRD_PARTY.md`

- [ ] **Step 1: Write failing lock completeness test**

```rust
#[test]
fn full_pack_lock_matches_audited_upstream() {
    let lock = GrammarPackLock::load(workspace_root().join("grammars/full-pack.toml")).unwrap();
    assert_eq!(lock.upstream_commit(), "2469ecc3a7a2f80debe296e1f17a1efcfdb9450c");
    assert_eq!(lock.grammars.len(), 159);
    assert_eq!(lock.language_mappings.len(), 160);
    assert_eq!(lock.abi_histogram(), BTreeMap::from([(13, 9), (14, 78), (15, 72)]));
    assert_eq!(lock.available_language_count(), 159);
    assert_eq!(lock.unique_bound_grammar_count(), 157);
    assert_eq!(lock.unavailable_language_ids(), ["nim"]);
    assert_eq!(
        lock.orphan_grammar_names(),
        ["objectscript_routine", "objectscript_udl"]
    );
    assert_eq!(lock.grammar_name_for("yaml").unwrap(), "yaml");
    assert_eq!(lock.grammar_name_for("kustomize").unwrap(), "yaml");
    assert_eq!(lock.grammar_name_for("k8s").unwrap(), "yaml");
    assert!(lock.grammars.iter().all(|g| !g.source_hash.is_empty()));
    assert!(lock.grammars.iter().all(|g| !g.license_files.is_empty()));
}
```

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p goldeneye-syntax --test grammar_lock`

Expected: FAIL because lock/export types do not exist.

- [ ] **Step 3: Implement lock schema, validation, and deterministic exporter**

`pack.rs` deserializes the TOML into owned records. Top-level metadata declares grammar count, language-binding count, compatible ABI range, and upstream commit; validation checks those declared counts plus unique names/IDs, relative slash-normalized paths, ABI compatibility, non-empty hashes, and non-empty license declarations. Every language binding is explicitly `available` with a grammar name or `unavailable` with a reason; every unbound grammar asset is explicitly marked orphaned with a reason. This keeps tiny test packs valid while the committed release lock test independently pins `159`, `160`, and the audited upstream commit. `xtask` depends on this shared model; it must not carry a second lock parser.

The audited upstream `MANIFEST.md` ABI summary is stale. Each grammar's direct `<grammar>/parser.c` must contain exactly one `LANGUAGE_VERSION` marker; those 159 generated parsers are authoritative: ABI 13 has 9 grammars, ABI 14 has 78, and ABI 15 has 72. The nested `rst/tree_sitter_rst/parser.c` is a scanner helper with no ABI marker: it is locked and copied as compilation source but excluded from the histogram. Upstream also has one detected language without a `ts_factory` (`nim`), three IDs sharing YAML (`yaml`, `kustomize`, `k8s`), and two unbound ObjectScript grammar assets. Therefore 159 active IDs resolve to 157 unique bound grammar assets. These are explicit lock states, never silent count exceptions.

`tools/export_grammar_lock.py` reads pinned upstream:

- `internal/cbm/vendored/grammars/MANIFEST.md`;
- every compilation asset under each grammar (`*.c`, `*.h`, and `*.inc`, including helper sources) plus the direct `LICENSE`;
- `crates/goldeneye-discovery/data/languages.tsv`;
- upstream grammar registry mappings.

All upstream metadata and asset bytes come from the exact expected commit's Git
tree (`ls-tree` plus streaming `cat-file --batch`), not mutable worktree file
paths. Git replacement-object resolution is disabled for every subprocess so
replacement refs cannot substitute another tree under the pinned commit ID.
Only regular Git blob modes are accepted for files the exporter reads.

It emits one TOML grammar record with name, pinned repository/commit (or an explicit reason when the audited manifest has no upstream revision), ABI read from the direct generated `parser.c`, relative asset paths, framed SHA-256 source hash, scanner language, license files, verdict, local-patch provenance, and optional explicit orphan reason. It emits 160 language bindings, including explicit unavailable entries. Output contains no timestamps or absolute paths and sorts every record/path/binding. It refuses ABI outside the runtime-compatible range, a missing/multiple direct ABI parser, missing license, count mismatch, implicit unavailable/orphan state, unresolved available binding, symlink/non-regular assets, or source outside grammar root.

Grammar hashing is exactly `SHA-256(ASCII("goldeneye-grammar-assets-v1") || 0x00 || repeated(u64_be(path_len) || slash_normalized_utf8_path || u64_be(content_len) || raw_content))` over every locked `*.c`, `*.h`, `*.inc`, and direct `LICENSE`, sorted by UTF-8 path bytes. `path_len` is the UTF-8 byte length and `content_len` is the raw byte length. Length framing prevents path/content concatenation ambiguity; non-UTF-8 or non-normalized paths are rejected.

- [ ] **Step 4: Implement explicit offline sync command**

Add `xtask` workspace member and workspace-local Cargo alias `xtask = "run -p xtask --"`. Provide `grammars verify` (hash/license/provenance only) and `grammars sync` (verify then materialize). Command:

```bash
cargo xtask grammars sync \
  --lock grammars/full-pack.toml \
  --source .upstream/codebase-memory-mcp/internal/cbm/vendored/grammars \
  --dest target/goldeneye-grammars
```

Behavior:

1. never accesses network;
2. canonicalizes source and the destination parent (plus destination when it exists);
3. rejects source/destination overlap in either direction;
4. rejects symlink/reparse or non-regular locked assets;
5. verifies every locked source hash/license before copy;
6. copies only the explicitly locked compilation assets (`*.c`, `*.h`, `*.inc`) and direct licenses;
7. returns a no-op when an existing destination has the same verified `pack-state.json`;
8. rejects an existing mismatched/non-pack destination without deleting or modifying it;
9. writes an absent destination through a temporary sibling then atomic rename;
10. writes `pack-state.json` with lock hash;
11. removes temporary output on failure.

- [ ] **Step 5: Add sync safety/reproducibility tests**

Use a tiny two-grammar fixture. Cover the hash framing golden, clean verify/sync, hash mismatch, missing license, traversal path, stale temp cleanup, identical existing-pack no-op, mismatched/non-pack destination rejection without mutation, deterministic repeated output, and no mutation of source.

- [ ] **Step 6: Update legal ledger**

Record Tree-sitter runtime and six core grammar crate licenses/versions. Record full lock provenance and require all grammar license files to travel with materialized/release packs.

- [ ] **Step 7: Run metadata/materialization gate against the real pinned checkout**

Run:

```bash
python tools/export_grammar_lock.py --check \
  --source .upstream/codebase-memory-mcp \
  --expected-commit 2469ecc3a7a2f80debe296e1f17a1efcfdb9450c \
  --output grammars/full-pack.toml
cargo xtask grammars verify \
  --lock grammars/full-pack.toml \
  --source .upstream/codebase-memory-mcp/internal/cbm/vendored/grammars
cargo xtask grammars sync \
  --lock grammars/full-pack.toml \
  --source .upstream/codebase-memory-mcp/internal/cbm/vendored/grammars \
  --dest target/goldeneye-grammars-audit
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: all commands exit 0; exporter is byte-for-byte reproducible; real pinned assets verify/materialize; six core runtime grammars and audited 159-asset/160-binding metadata pass. This remains pre-GFP evidence, not full provider/release parity.

- [ ] **Step 8: Commit**

```bash
git add .cargo/config.toml Cargo.toml Cargo.lock crates/goldeneye-syntax grammars tools/export_grammar_lock.py xtask THIRD_PARTY.md docs/superfastpowers/plans/GS
git commit -m "[GS-5] build: lock full Tree-sitter grammar pack"
```
