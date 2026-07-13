# Context for GS-5

**Plan:** `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core.md`
**Task:** `GS-5`
**Plan Commit SHA:** `4305d0c`

## Starting Context

- `grammars/full-pack.toml`: starting point named by implementation plan.
- `tools/export_grammar_lock.py`: starting point named by implementation plan.
- `.cargo/config.toml`: starting point named by implementation plan.
- `xtask/Cargo.toml`: starting point named by implementation plan.
- `xtask/src/main.rs`: starting point named by implementation plan.
- `xtask/tests/grammar_sync.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/pack.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/Cargo.toml`: starting point named by implementation plan.
- `crates/goldeneye-syntax/tests/grammar_lock.rs`: starting point named by implementation plan.
- `Cargo.toml`: starting point named by implementation plan.
- `Cargo.lock`: starting point named by implementation plan.
- `THIRD_PARTY.md`: starting point named by implementation plan.
- `rust
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
`: starting point named by implementation plan.
- `

- [ ] **Step 2: Run test and verify RED**

Run: `: starting point named by implementation plan.
- `

Expected: FAIL because lock/export types do not exist.

- [ ] **Step 3: Implement lock schema, validation, and deterministic exporter**

`: starting point named by implementation plan.
- ` deserializes the TOML into owned records. Top-level metadata declares grammar count, language-binding count, compatible ABI range, and upstream commit; validation checks those declared counts plus unique names/IDs, relative slash-normalized paths, ABI compatibility, non-empty hashes, and non-empty license declarations. Every language binding is explicitly `: starting point named by implementation plan.
- ` with a grammar name or `: starting point named by implementation plan.
- ` with a reason; every unbound grammar asset is explicitly marked orphaned with a reason. This keeps tiny test packs valid while the committed release lock test independently pins `: starting point named by implementation plan.
- `, `: starting point named by implementation plan.
- `, and the audited upstream commit. `: starting point named by implementation plan.
- ` depends on this shared model; it must not carry a second lock parser.

The audited upstream `: starting point named by implementation plan.
- ` ABI summary is stale. Generated `: starting point named by implementation plan.
- ` is authoritative: ABI 13 has 9 grammars, ABI 14 has 78, and ABI 15 has 72. Upstream also has one detected language without a `: starting point named by implementation plan.
- ` (`: starting point named by implementation plan.
- `), three IDs sharing YAML (`: starting point named by implementation plan.
- `, `: starting point named by implementation plan.
- `, `: starting point named by implementation plan.
- `), and two unbound ObjectScript grammar assets. Therefore 159 active IDs resolve to 157 unique bound grammar assets. These are explicit lock states, never silent count exceptions.

`: starting point named by implementation plan.
- ` reads pinned upstream:

- `: starting point named by implementation plan.
- `;
- all parser/scanner/header assets;
- `: starting point named by implementation plan.
- `;
- upstream grammar registry mappings.

It emits one TOML grammar record with name, pinned repository/commit, ABI read from each generated `: starting point named by implementation plan.
- `, relative asset paths, framed SHA-256 source hash, scanner language, license files, verdict, and optional explicit orphan reason. It emits 160 language bindings, including explicit unavailable entries. Output contains no timestamps or absolute paths and sorts every record/path/binding. It refuses ABI outside the runtime-compatible range, missing license, count mismatch, implicit unavailable/orphan state, unresolved available binding, symlink/non-regular assets, or source outside grammar root.

Grammar hashing is exactly `: starting point named by implementation plan.
- ` over every copied parser/scanner/header/license asset sorted by path bytes. Length framing prevents path/content concatenation ambiguity; non-UTF-8 or non-normalized paths are rejected.

- [ ] **Step 4: Implement explicit offline sync command**

Add `: starting point named by implementation plan.
- ` workspace member and workspace-local Cargo alias `: starting point named by implementation plan.
- `. Provide `: starting point named by implementation plan.
- ` (hash/license/provenance only) and `: starting point named by implementation plan.
- ` (verify then materialize). Command:

`: starting point named by implementation plan.
- `bash
cargo xtask grammars sync \
  --lock grammars/full-pack.toml \
  --source .upstream/codebase-memory-mcp/internal/cbm/vendored/grammars \
  --dest target/goldeneye-grammars
`: starting point named by implementation plan.
- `

Behavior:

1. never accesses network;
2. canonicalizes source and the destination parent (plus destination when it exists);
3. rejects source/destination overlap in either direction;
4. rejects symlink/reparse or non-regular locked assets;
5. verifies every locked source hash/license before copy;
6. copies only locked parser/scanner/header/license assets;
7. returns a no-op when an existing destination has the same verified `: starting point named by implementation plan.
- `;
8. rejects an existing mismatched/non-pack destination without deleting or modifying it;
9. writes an absent destination through a temporary sibling then atomic rename;
10. writes `: starting point named by implementation plan.
- ` with lock hash;
11. removes temporary output on failure.

- [ ] **Step 5: Add sync safety/reproducibility tests**

Use a tiny two-grammar fixture. Cover the hash framing golden, clean verify/sync, hash mismatch, missing license, traversal path, stale temp cleanup, identical existing-pack no-op, mismatched/non-pack destination rejection without mutation, deterministic repeated output, and no mutation of source.

- [ ] **Step 6: Update legal ledger**

Record Tree-sitter runtime and six core grammar crate licenses/versions. Record full lock provenance and require all grammar license files to travel with materialized/release packs.

- [ ] **Step 7: Run metadata/materialization gate against the real pinned checkout**

Run:

`: starting point named by implementation plan.
- `bash
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
`: starting point named by implementation plan.
- `

Expected: all commands exit 0; exporter is byte-for-byte reproducible; real pinned assets verify/materialize; six core runtime grammars and audited 159-asset/160-binding metadata pass. This remains pre-GFP evidence, not full provider/release parity.

- [ ] **Step 8: Commit**

`: starting point named by implementation plan.
- `bash
git add .cargo/config.toml Cargo.toml Cargo.lock crates/goldeneye-syntax grammars tools/export_grammar_lock.py xtask THIRD_PARTY.md docs/superfastpowers/plans/GS
git commit -m "[GS-5] build: lock full Tree-sitter grammar pack"
`: starting point named by implementation plan.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Pending implementation, review evidence, final commit, and controller verification.
