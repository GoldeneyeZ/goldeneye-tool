# Goldeneye Full Grammar Provider Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superfastpowers:goal-driven-development and superfastpowers:test-driven-development task-by-task, with superfastpowers:requesting-code-review and superfastpowers:verification-before-completion at each gate. A worker dispatched with one precise task applies the reloaded `<SUBAGENT-STOP>` bypass to `using-superfastpowers`; the task-specific TDD and review gates still apply. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build an offline, source-verified full Tree-sitter provider covering the pinned upstream registry: 160 declared IDs, 159 callable IDs, 157 unique runtime grammars, one typed-unavailable Nim binding, and two compile-only ObjectScript orphan assets.

**Architecture:** Extract grammar-pack integrity into `goldeneye-grammar-pack`; generate exact callable metadata from the checked-in lock; compile locked C wrappers only inside `goldeneye-full-grammars`; adapt its safe `LanguageFn` lookup to `GrammarProvider` in `goldeneye-syntax`. Every full-pack factory/scanner symbol is prefixed with `goldeneye_full_`, making additive core+full Cargo features link-safe; official full builds still disable core defaults to avoid redundant code. Default CI stays cache-free; an explicit offline full-pack lane materializes, compiles, links, and probes the pack.

**Plan Acronym:** GFP

**Design:** `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`

**Tech Stack:** Rust 1.97.0, Cargo build scripts and feature resolution, `tree-sitter 0.26.11`, `tree-sitter-language 0.1.7`, `cc`, streamed SHA-256 verification, Python lock exporter, GitHub Actions, C11/MSVC.

---

## File Structure

- `crates/goldeneye-grammar-pack/Cargo.toml`: safe lock/materialized-pack crate dependencies.
- `crates/goldeneye-grammar-pack/src/lib.rs`: moved lock schema, source verification, pack-state and exact-layout verification.
- `crates/goldeneye-grammar-pack/src/git_source.rs`: moved exact-Git object protocol.
- `crates/goldeneye-grammar-pack/tests/materialized_pack.rs`: pack-state, layout, hash, symlink, and extra-file behavior.
- `crates/goldeneye-syntax/Cargo.toml`: core/full provider features and full-only dependency wiring.
- `crates/goldeneye-syntax/src/grammar.rs`: feature-gated core provider and ABI mismatch error.
- `crates/goldeneye-syntax/src/full_grammar.rs`: safe `FullGrammarProvider` adapter.
- `crates/goldeneye-syntax/src/lib.rs`: compatibility re-exports and feature-gated providers.
- `crates/goldeneye-syntax/tests/full_grammars.rs`: full registry, link, ABI, parse, helper-layout, alias, and concurrency tests.
- `crates/goldeneye-full-grammars/Cargo.toml`: default-empty/compiled native crate features and dependencies.
- `crates/goldeneye-full-grammars/build.rs`: verified-cache validation and per-grammar C compilation.
- `crates/goldeneye-full-grammars/src/lib.rs`: safe compiled-registry API with confined generated FFI module.
- `crates/goldeneye-full-grammars/src/generated.rs`: deterministic checked-in registry and exact factory declarations.
- `crates/goldeneye-full-grammars/tests/compiled_registry.rs`: native registry/link smoke tests.
- `grammars/full-pack.toml`: persisted `exported_symbol` for every grammar record.
- `grammars/full-pack-license-ledger.md`: deterministic 159-record notice/provenance ledger.
- `tools/export_grammar_lock.py`: streamed factory extraction and upstream cross-check.
- `tools/test_export_grammar_lock.py`: boundary, duplicate, malformed, and exception tests.
- `xtask/Cargo.toml`: direct pack-crate dependency.
- `xtask/src/lib.rs`: atomic sync through shared pack-state API and deterministic provider generation.
- `xtask/src/main.rs`: `grammars generate-provider` command and `--check` mode.
- `xtask/tests/grammar_sync.rs`: shared materialized-pack verification regression coverage.
- `xtask/tests/provider_generation.rs`: deterministic generated registry and mapping tests.
- `xtask/tests/full_pack_ci.rs`: workflow contract assertions.
- `.github/workflows/ci.yml`: default three-platform lane plus explicit Linux full-pack lane.
- `docs/full-grammar-pack.md`: local sync/build/test instructions and provider-flavor contract.
- `THIRD_PARTY.md`: exact native-input and completion-claim language.

---

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

### Task 2: Persist Factory Symbols and Generate the Exact Registry

<TASK-ID>GFP-2</TASK-ID>

**Files:**
- Modify: `grammars/full-pack.toml`
- Create: `grammars/full-pack-license-ledger.md`
- Modify: `tools/export_grammar_lock.py`
- Modify: `tools/test_export_grammar_lock.py`
- Modify: `crates/goldeneye-grammar-pack/src/lib.rs`
- Modify: `crates/goldeneye-syntax/tests/grammar_lock.rs`
- Create: `crates/goldeneye-full-grammars/Cargo.toml`
- Create: `crates/goldeneye-full-grammars/src/lib.rs`
- Create: `crates/goldeneye-full-grammars/src/generated.rs`
- Modify: `xtask/src/lib.rs`
- Modify: `xtask/src/main.rs`
- Create: `xtask/tests/provider_generation.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing streamed factory-extraction tests**

Add Python tests that require extraction of exactly one direct definition matching the generated parser entry form, including:

- every byte split through `tree_sitter_COBOL` and its surrounding declaration;
- a symbol split at the stream boundary;
- a symbol at EOF without a final newline;
- scanner prototypes that must not be mistaken for the language factory;
- duplicate direct language factories;
- malformed/non-identifier symbols;
- all eight factory exceptions;
- the 151 conventional `tree_sitter_<grammar>` cases.

The extractor must stream and hash the original bytes; it may not read the 104 MiB parser into one buffer.

- [ ] **Step 2: Run exporter tests and verify RED**

Run:

```text
python tools/test_export_grammar_lock.py
```

Expected: FAIL because grammar records do not expose `exported_symbol` and the extractor does not return it.

- [ ] **Step 3: Implement symbol extraction, cross-checks, and lock validation**

Add `exported_symbol` to every emitted grammar record. For bound grammars, cross-check it against the factory joined from upstream `CBMLanguage`/`lang_specs.c`; for orphans, use the direct parser definition only. Reject duplicates globally.

In Rust, validate:

- prefix `tree_sitter_`;
- ASCII C identifier syntax;
- global uniqueness across 159 records;
- no missing symbol;
- no mapping to an orphan;
- explicit supported scanner languages `none` and `c` only.

Add exact tests for both normalization tables and the case-sensitive COBOL symbol.

- [ ] **Step 4: Regenerate and reproduce the real lock**

Run:

```text
python tools/export_grammar_lock.py \
  --source .upstream/codebase-memory-mcp \
  --expected-commit 2469ecc3a7a2f80debe296e1f17a1efcfdb9450c \
  --output grammars/full-pack.toml

python tools/export_grammar_lock.py --check \
  --source .upstream/codebase-memory-mcp \
  --expected-commit 2469ecc3a7a2f80debe296e1f17a1efcfdb9450c \
  --output grammars/full-pack.toml
```

Expected: generation succeeds, `--check` reports no drift, asset hashes/counts and ABI histograms remain unchanged.

- [ ] **Step 5: Write failing provider-generation tests**

Tests must require deterministic Rust output with:

- 160 lexically sorted ID records;
- 159 callable records and typed-unavailable `nim`;
- 157 unique callable grammar records;
- three YAML-family ID rows sharing one grammar index;
- no ObjectScript runtime entry;
- ordinal Rust extern identifiers with exact `#[link_name = "goldeneye_full_..."]` values derived from locked upstream factories;
- embedded lock hash and locked ABI/source hashes;
- stable byte-for-byte output on repeated generation;
- safe Rust string escaping for every lock-controlled field;
- a separate deterministic license ledger with 159 rows, one direct license path per grammar, and exact repository/revision-or-reason/source-hash metadata.

Run:

```text
cargo test -p xtask --test provider_generation
```

Expected: FAIL because the command/renderer and full-grammar crate do not exist.

- [ ] **Step 6: Implement the deterministic generator and default-empty crate**

Add:

```text
cargo xtask grammars generate-provider \
  --lock grammars/full-pack.toml \
  --output crates/goldeneye-full-grammars/src/generated.rs
```

`--check` renders in memory and fails on any byte difference without writing. The checked-in file begins with `// goldeneye-full-pack-lock-sha256: <64 lowercase hex>` and otherwise has no timestamp, no absolute path, and no upstream-order dependence.

Add `cargo xtask grammars generate-notices` with matching `--lock`, `--output`, and `--check` behavior. It produces one lexically sorted row per grammar containing repository, revision or missing-revision reason, direct license path, and source hash.

Create `goldeneye-full-grammars` with default features empty. `src/lib.rs` must compile without loading `generated.rs` or requiring a cache. The compiled API and generated module remain feature-gated for GFP-3.

- [ ] **Step 7: Run task gates**

Run:

```text
python tools/test_export_grammar_lock.py
python tools/export_grammar_lock.py --check --source .upstream/codebase-memory-mcp --expected-commit 2469ecc3a7a2f80debe296e1f17a1efcfdb9450c --output grammars/full-pack.toml
cargo xtask grammars generate-provider --lock grammars/full-pack.toml --output crates/goldeneye-full-grammars/src/generated.rs --check
cargo xtask grammars generate-notices --lock grammars/full-pack.toml --output grammars/full-pack-license-ledger.md --check
cargo test -p goldeneye-grammar-pack
cargo test -p goldeneye-syntax --test grammar_lock
cargo test -p xtask --test provider_generation
cargo check -p goldeneye-full-grammars
cargo test --workspace
git diff --check
```

- [ ] **Step 8: Commit**

```text
git add Cargo.lock grammars tools crates/goldeneye-grammar-pack crates/goldeneye-full-grammars crates/goldeneye-syntax/tests/grammar_lock.rs xtask
git commit -m "[GFP-2] feat: generate exact full grammar registry"
```

---

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

### Task 4: Add the Safe Full `GrammarProvider` and Runtime Audit

<TASK-ID>GFP-4</TASK-ID>

**Files:**
- Modify: `crates/goldeneye-syntax/Cargo.toml`
- Modify: `crates/goldeneye-syntax/src/grammar.rs`
- Create: `crates/goldeneye-syntax/src/full_grammar.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Modify: `crates/goldeneye-syntax/tests/core_grammars.rs`
- Modify: `crates/goldeneye-syntax/tests/diagnostics.rs`
- Modify: `crates/goldeneye-syntax/tests/inspect.rs`
- Modify: `crates/goldeneye-syntax/tests/locators.rs`
- Create: `crates/goldeneye-syntax/tests/full_grammars.rs`
- Modify: `Cargo.lock`

- [ ] **Step 1: Write failing full-provider contract tests**

Under `full-grammar-pack`, require:

- 160 declared registry rows, 159 supported IDs, 157 unique grammars, one unavailable ID, two orphans;
- exact language/grammar and grammar/factory exception tables;
- `nim` and unknown IDs return typed `UnsupportedGrammar`;
- ObjectScript is absent from every runtime query;
- YAML/K8s/Kustomize retain three IDs but use equal Tree-sitter languages;
- every supported lookup returns `GrammarSource::FullPack` with the locked name/hash;
- every callable language ABI equals its locked ABI and fits 13 through 15;
- each language passes `Parser::set_language` and returns a tree for empty input;
- non-empty fixtures parse for Crystal, RST, YAML, VHDL, FSharp, QML, PureScript, and ReScript;
- concurrent lookups succeed and `FullGrammarProvider: Send + Sync`;
- an all-features test binary links and exercises both core and full Rust/YAML-family lookups without duplicate symbols.

- [ ] **Step 2: Run the full-provider test and verify RED**

Run:

```text
$env:GOLDENEYE_GRAMMAR_PACK_DIR = "target/goldeneye-grammars"
$env:CARGO_NET_OFFLINE = "true"
cargo test -p goldeneye-syntax --test full_grammars \
  --no-default-features --features full-grammar-pack
```

Expected: FAIL because the feature and provider do not exist.

- [ ] **Step 3: Make core and full features explicit and link-safe**

Make all five core grammar dependencies optional and enable them through default feature `core-grammars`. Gate `CoreGrammarProvider` and its re-export.

Add optional full dependency with `features = ["compiled"]`, then expose it through `full-grammar-pack`. Do not reject simultaneous activation: prefixed full-pack symbols make the mixed graph safe, and GFP uses that graph as a collision sentinel.

Gate existing core-provider integration tests with `core-grammars` so the full-only lane does not link core crates. Keep default behavior unchanged; all-features runs both provider suites.

- [ ] **Step 4: Implement `FullGrammarProvider` and ABI drift errors**

Add a typed `SyntaxError::GrammarAbiMismatch { language_id, expected, actual }` or an equivalently precise variant.

`FullGrammarProvider::grammar` must:

1. lookup the exact ID in the safe native registry;
2. convert `LanguageFn` through the safe Tree-sitter conversion;
3. checked-convert `abi_version()` to `u32`;
4. reject any mismatch from locked ABI;
5. return `Grammar` with the requested ID and full-pack provenance.

`supported_ids` returns the generated 159-ID lexical list. It does not reconstruct or sort mappings at runtime beyond creating the trait-required `Vec<LanguageId>`.

- [ ] **Step 5: Run full-only runtime and mixed-link gates**

Run:

```text
$env:GOLDENEYE_GRAMMAR_PACK_DIR = "target/goldeneye-grammars"
$env:CARGO_NET_OFFLINE = "true"
cargo test -p goldeneye-syntax --no-default-features --features full-grammar-pack
cargo clippy -p goldeneye-syntax --all-targets --no-default-features --features full-grammar-pack -- -D warnings
```

Then run the mixed collision sentinel:

```text
cargo test -p goldeneye-syntax --all-features
cargo tree -p goldeneye-syntax --no-default-features --features full-grammar-pack -e features
```

Expected: the mixed test links and passes because every full symbol is prefixed. The full-only feature tree contains `goldeneye-full-grammars` but none of the five maintained core grammar crates.

- [ ] **Step 6: Rerun the complete default lane after clearing full-pack state**

Run:

```text
Remove-Item Env:GOLDENEYE_GRAMMAR_PACK_DIR -ErrorAction SilentlyContinue
Remove-Item Env:CARGO_NET_OFFLINE -ErrorAction SilentlyContinue
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace --release
git diff --check
```

Expected: all pass without touching the full cache.

- [ ] **Step 7: Commit**

```text
git add Cargo.lock crates/goldeneye-syntax
git commit -m "[GFP-4] feat: expose full grammar provider"
```

---

### Task 5: Add Offline Full-Pack CI, Operator Documentation, and Claim Guards

<TASK-ID>GFP-5</TASK-ID>

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `docs/full-grammar-pack.md`
- Modify: `THIRD_PARTY.md`
- Create: `xtask/tests/full_pack_ci.rs`

- [ ] **Step 1: Write failing CI/documentation contract tests**

The test reads tracked text and requires:

- the existing Linux/Windows/macOS default matrix still runs core-only workspace Clippy/tests;
- a Linux full-pack job checks out exact upstream commit `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`;
- dependency/upstream acquisition precedes the offline boundary;
- exporter `--check`, Git sync, materialized verification, provider generation `--check`, compiled-crate tests, and full syntax tests are present;
- license-ledger generation `--check`, mixed core+full link testing, and a full-only Cargo feature-tree sentinel are present;
- full Cargo work uses `CARGO_NET_OFFLINE=true` and the explicit cache variable;
- docs state `160/159/157/2`, symbol namespacing, full-only artifact features, no build-time downloads, and the Phase 6 packaging boundary;
- `THIRD_PARTY.md` never claims a core-only build is 160-language evidence.

Run:

```text
cargo test -p xtask --test full_pack_ci
```

Expected: FAIL because the full job and operator document do not exist.

- [ ] **Step 2: Run the CI contract test and verify RED**

Run:

```text
cargo test -p xtask --test full_pack_ci
```

Expected: FAIL on the missing full-pack workflow and documentation assertions.

- [ ] **Step 3: Implement the explicit Linux full-pack job**

Keep the default matrix unchanged. Add a separate Linux job that:

1. checks out Goldeneye;
2. checks out the exact upstream SHA into `.upstream/codebase-memory-mcp`;
3. installs Rust 1.97.0 with Clippy/Rustfmt;
4. runs `cargo fetch --locked` while network is allowed;
5. reproduces the lock, provider registry, and 159-entry license ledger;
6. materializes and re-verifies `target/goldeneye-grammars`;
7. sets `CARGO_NET_OFFLINE=true`;
8. runs compiled native and full-only syntax Clippy/tests, then the mixed core+full link sentinel;
9. reruns a cache-free default check or depends on the default matrix.

Do not use broad cache restore keys or treat a cache hit as verification. A future exact-key native cache may include OS/architecture, Rust version, compiler identity, `Cargo.lock`, lock hash, and generator hash.

- [ ] **Step 4: Document local operation and third-party boundaries**

`docs/full-grammar-pack.md` contains copyable PowerShell and POSIX forms for:

- exact upstream acquisition;
- Git-backed sync;
- materialized verification;
- registry `--check`;
- default versus full feature commands;
- missing/stale cache recovery;
- expected cardinalities and provider claims.

Update `THIRD_PARTY.md` to distinguish verified metadata/materialization, compiled GFP evidence, and Phase 6 release-license bundling. State that no upstream application C or bundled Tree-sitter runtime is linked.

- [ ] **Step 5: Run CI contract and regeneration gates**

Run:

```text
cargo test -p xtask --test full_pack_ci
python tools/test_export_grammar_lock.py
python tools/export_grammar_lock.py --check --source .upstream/codebase-memory-mcp --expected-commit 2469ecc3a7a2f80debe296e1f17a1efcfdb9450c --output grammars/full-pack.toml
cargo xtask grammars generate-provider --lock grammars/full-pack.toml --output crates/goldeneye-full-grammars/src/generated.rs --check
cargo xtask grammars generate-notices --lock grammars/full-pack.toml --output grammars/full-pack-license-ledger.md --check
```

- [ ] **Step 6: Run fresh default and full-pack integration gates**

Run default first, full second, then default again:

```text
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

$env:GOLDENEYE_GRAMMAR_PACK_DIR = "target/goldeneye-grammars"
$env:CARGO_NET_OFFLINE = "true"
cargo clippy -p goldeneye-full-grammars --all-targets --features compiled -- -D warnings
cargo test -p goldeneye-full-grammars --features compiled
cargo clippy -p goldeneye-syntax --all-targets --no-default-features --features full-grammar-pack -- -D warnings
cargo test -p goldeneye-syntax --no-default-features --features full-grammar-pack
cargo test -p goldeneye-syntax --all-features
cargo tree -p goldeneye-syntax --no-default-features --features full-grammar-pack -e features
cargo test -p goldeneye-syntax --release --no-default-features --features full-grammar-pack --no-run

Remove-Item Env:GOLDENEYE_GRAMMAR_PACK_DIR -ErrorAction SilentlyContinue
Remove-Item Env:CARGO_NET_OFFLINE -ErrorAction SilentlyContinue
cargo test --workspace
git diff --check
```

Record elapsed time, source-cache size, target size, and linked test-binary size as observational baselines; do not add unsupported release ceilings in GFP.

- [ ] **Step 7: Commit**

```text
git add .github/workflows/ci.yml docs/full-grammar-pack.md THIRD_PARTY.md xtask/tests/full_pack_ci.rs
git commit -m "[GFP-5] ci: verify offline full grammar provider"
```

---

## Goal-Level Final Integration Review

After GFP-1 through GFP-5 have complete implementer, spec-review, and code-quality records:

1. Review the entire plan range, not task commits in isolation.
2. Re-audit feature unification to prove core and prefixed full symbols coexist without collision, while the full-only graph excludes core grammar crates.
3. Reproduce both checked-in generators from the pinned upstream snapshot.
4. Verify the materialized cache from scratch or from an independently reverified exact source.
5. Run the default/full/default gate sequence from GFP-5.
6. Inspect the full linked test binary or map for all 157 callable factories and absence of both ObjectScript factories.
7. Confirm full-provider metadata, ABI, and parse probes for all 159 supported IDs.
8. Confirm default workspace commands never require the full cache.
9. Run `git diff --check`, confirm `git status --short` is empty, and create `final-review.md` with the reviewed range and exact evidence.

GFP may be marked complete only when the independent final reviewer reports checked with no unresolved findings. Then continue to the graph/store/index phase; do not treat GFP as completion of the overall Rust port.
