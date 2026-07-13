### Task 3: Compile the Verified Native Grammar Pack Behind an Opt-In Feature

<TASK-ID>GFP-3</TASK-ID>

**Files:**
- Modify: `crates/goldeneye-full-grammars/Cargo.toml`
- Create: `crates/goldeneye-full-grammars/build.rs`
- Modify: `crates/goldeneye-full-grammars/src/lib.rs`
- Create: `crates/goldeneye-full-grammars/tests/compiled_registry.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing opt-in/build-plan tests**

Require these behaviors:

- `cargo check -p goldeneye-full-grammars` succeeds with no cache or environment variable;
- the generated native plan has 159 wrappers: 57 parser-only and 102 with a root C scanner;
- it exposes 157 factories and excludes both ObjectScript records from lookup;
- helper `.c` files are never independent compilation units;
- every wrapper aliases the locked factory and, when present, all five standard scanner exports to `goldeneye_full_*`;
- unsupported scanner languages fail before compiler invocation;
- enabling `compiled` without `GOLDENEYE_GRAMMAR_PACK_DIR` fails with remediation containing `cargo xtask grammars sync`;
- a stale state, extra file, or hash drift fails before wrapper creation;
- two generated wrapper passes are byte-identical.

The missing-cache command is an expected-failure gate, not a test that mutates the process environment concurrently.

- [ ] **Step 2: Run focused tests and verify RED**

Run:

```text
cargo test -p goldeneye-full-grammars --test compiled_registry
```

Expected: FAIL because the compiled feature, build script, and safe native lookup do not exist.

- [ ] **Step 3: Implement cache verification and deterministic wrappers**

Add `default = []` and feature `compiled`. Without it, `build.rs` emits only feature/env rerun metadata and returns without reading the cache variable, lock, or filesystem cache.

With it, the build must:

1. require and resolve `GOLDENEYE_GRAMMAR_PACK_DIR`;
2. call the shared `verify_materialized_pack` before compiling;
3. parse the generated file's strict first-line lock-hash header and compare it to the active lock;
4. emit rerun metadata for the environment, lock, state, and 907 assets;
5. create one wrapper in `OUT_DIR` for each of 159 grammar records, defining `goldeneye_full_` aliases for the exact locked factory and the five standard external-scanner functions before including parser/scanner source;
6. add the verified pack root through `cc::Build::include`;
7. compile each wrapper as its own deterministically named static archive;
8. avoid whole-archive flags.

Use C11, `_DEFAULT_SOURCE`, disabled generated-source warnings, and target-aware MSVC UTF-8/large-object flags via supported-flag checks. Do not copy, flatten, patch, or fetch source during build.

- [ ] **Step 4: Confine the FFI boundary and expose a safe lookup**

This crate must not use `[lints] workspace = true`, because workspace `unsafe_code = "forbid"` cannot be lowered. Its manifest repeats `clippy.all = "deny"` and `clippy.pedantic = "deny"`, sets local Rust `unsafe_code = "deny"`, and permits only this module:

```rust
#[cfg(feature = "compiled")]
#[allow(unsafe_code)]
mod generated;
```

The generated module owns `unsafe extern "C"` declarations, prefixed `#[link_name]` attributes, and `LanguageFn::from_raw`. Public APIs return a copied `LanguageFn` and immutable metadata by safe value/reference; raw functions and pointers remain private.

Expose static queries for all 160 declared records, the 159 available IDs, 157 unique grammars, the compiled-source count, and the embedded lock hash. Lookup must use binary search, not a runtime hash map.

- [ ] **Step 5: Materialize a current cache and run the full native gate**

Use a fresh destination because the new lock hash includes `exported_symbol`:

```text
cargo xtask grammars sync \
  --lock grammars/full-pack.toml \
  --git-repo .upstream/codebase-memory-mcp \
  --git-prefix internal/cbm/vendored/grammars \
  --dest target/goldeneye-grammars
```

Then run:

```text
$env:GOLDENEYE_GRAMMAR_PACK_DIR = "target/goldeneye-grammars"
$env:CARGO_NET_OFFLINE = "true"
cargo test -p goldeneye-full-grammars --features compiled
```

Expected: all 159 wrappers compile on MSVC; all 157 generated factory references link; registry tests pass.

- [ ] **Step 6: Prove negative and helper-layout behavior**

Run an expected-failure build against a missing cache and confirm the remediation. In the successful build/test output, prove targeted compilation/link behavior for Crystal, RST, YAML-core, VHDL, FSharp, and QML. Assert the two ObjectScript factory symbols are absent from the linked test binary or linker map.

Do not infer 159 upstream wrapper files or 102 total filenames named `scanner.c`: GFP synthesizes the two orphan wrappers and RST contains a nested helper scanner.

- [ ] **Step 7: Run default-lane regression gates**

Clear the full-pack environment and run:

```text
cargo check -p goldeneye-full-grammars
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
git diff --check
```

Expected: no cache access and all default tests pass.

- [ ] **Step 8: Commit**

```text
git add Cargo.toml Cargo.lock crates/goldeneye-full-grammars
git commit -m "[GFP-3] feat: compile verified full grammar pack"
```

---
