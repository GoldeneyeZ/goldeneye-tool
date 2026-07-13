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
