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
