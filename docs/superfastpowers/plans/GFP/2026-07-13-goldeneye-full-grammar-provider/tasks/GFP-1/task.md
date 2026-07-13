### Task 1: Extract Grammar-Pack Integrity into a Build-Safe Crate

<TASK-ID>GFP-1</TASK-ID>

**Files:**
- Create: `crates/goldeneye-grammar-pack/Cargo.toml`
- Create: `crates/goldeneye-grammar-pack/src/lib.rs`
- Create: `crates/goldeneye-grammar-pack/src/git_source.rs`
- Create: `crates/goldeneye-grammar-pack/tests/materialized_pack.rs`
- Modify: `crates/goldeneye-syntax/Cargo.toml`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Delete: `crates/goldeneye-syntax/src/pack.rs`
- Delete: `crates/goldeneye-syntax/src/pack/git_source.rs`
- Modify: `xtask/Cargo.toml`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/tests/grammar_sync.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing crate-boundary and materialized-state tests**

Create tests that import the future crate directly and prove syntax compatibility re-exports remain the same types:

```rust
use goldeneye_grammar_pack::{GrammarPackLock, GrammarPackState};

#[test]
fn syntax_reexport_is_the_pack_crate_type() {
    fn accepts(_: goldeneye_syntax::GrammarPackLock) {}
    let lock = GrammarPackLock::load(workspace_root().join("grammars/full-pack.toml")).unwrap();
    accepts(lock);
}
```

Add materialized-pack fixtures covering:

- an exact state/layout/hash match;
- a mismatched lock hash;
- invalid JSON or unknown state fields;
- one missing asset;
- one extra file or directory;
- a final symlink and an intermediate-directory symlink;
- a same-size modified asset;
- a valid cache verified twice without mutation.

The public read-only API must expose `PACK_STATE_FILE`, `GrammarPackState::expected`, and `verify_materialized_pack` without exposing atomic replacement operations.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```text
cargo test -p goldeneye-grammar-pack --test materialized_pack
cargo test -p goldeneye-syntax --test grammar_lock
```

Expected: the first command cannot select/import the missing crate, and the compatibility test cannot use its types.

- [ ] **Step 3: Move the existing lock and Git verifier without behavior drift**

Create `goldeneye-grammar-pack` with the current safe dependencies. Move the implementation and internal tests rather than copying it. Preserve:

- `deny_unknown_fields` on all serialized records;
- path-component and identifier validation;
- direct `LICENSE` and `parser.c` rules;
- streamed one-handle hashing/copy behavior;
- exact Git commit and `cat-file --batch` protections;
- 159 grammar / 160 mapping / 907 asset semantics;
- domain-separated lock and asset hashes.

Do not add Tree-sitter, MCP, syntax-engine, or filesystem-mutation dependencies.

- [ ] **Step 4: Move pack-state and exact-layout verification down from `xtask`**

Define a serializable `GrammarPackState` with the existing schema:

```rust
pub struct GrammarPackState {
    schema_version: u32,
    lock_hash: String,
    upstream_commit: String,
    grammar_count: usize,
    asset_count: usize,
}
```

`GrammarPackState::expected(lock_path, lock)` computes, never accepts, these facts. `verify_materialized_pack(lock_path, lock, root)` must:

1. read a regular `pack-state.json`;
2. compare it to `expected`;
3. verify the exact file/directory set;
4. stream-verify every locked asset;
5. return `VerifiedPack` only after all checks pass.

Keep state-file writing and atomic publication in `xtask`. Reuse the shared expected-state and verification functions there; remove the duplicate private schema/layout code.

- [ ] **Step 5: Rewire consumers and preserve the syntax public API**

Make `goldeneye-syntax` depend on and publicly re-export:

```rust
pub use goldeneye_grammar_pack::{
    lock_file_hash, verify_materialized_pack, GrammarPackLock, GrammarPackState,
    GrammarRecord, LanguageBindingStatus, LanguageMapping, PackError, VerifiedPack,
};
```

Make `xtask` import the pack crate directly. Existing `grammar_lock` and `grammar_sync` tests must remain meaningful and green; no compatibility type may be duplicated.

- [ ] **Step 6: Run task gates**

Run:

```text
cargo fmt --check
cargo clippy -p goldeneye-grammar-pack -p goldeneye-syntax -p xtask --all-targets -- -D warnings
cargo test -p goldeneye-grammar-pack
cargo test -p goldeneye-syntax --test grammar_lock
cargo test -p xtask --test grammar_sync
cargo test --workspace
git diff --check
```

Expected: all pass; the default workspace still requires no grammar cache.

- [ ] **Step 7: Commit**

```text
git add Cargo.toml Cargo.lock crates/goldeneye-grammar-pack crates/goldeneye-syntax xtask
git commit -m "[GFP-1] refactor: extract grammar pack integrity crate"
```

---
