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
