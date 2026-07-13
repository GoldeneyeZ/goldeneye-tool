# Goldeneye Full Grammar Provider Design

**Status:** Approved by the parent Rust-port design and the user's instruction to proceed  
**Date:** 2026-07-13  
**Phase acronym:** GFP  
**Parent design:** `docs/superfastpowers/specs/2026-07-13-goldeneye-rust-port-design.md`

## Goal

Deliver the offline, source-built Tree-sitter provider required by the Rust port. The provider must make every upstream language binding explicit, compile the pinned native grammar assets reproducibly, keep ordinary development builds fast, and expose only safe Rust APIs to the syntax engine.

GFP is complete when:

- the checked-in registry describes all 160 upstream language IDs;
- 159 IDs are callable, `nim` is explicitly unavailable, and those IDs resolve to 157 unique runtime grammars;
- all 159 locked grammar asset records are verified and compiled, including the two unbound ObjectScript assets as compile-only evidence;
- the two ObjectScript assets are not reachable from the runtime registry;
- a full-provider test binary links and exercises all 157 callable grammar factories;
- builds perform no implicit network access;
- default workspace tests remain core-only and require no full grammar cache;
- full-pack native symbols are namespaced so additive Cargo feature unification cannot collide with maintained core-grammar symbols.

This is a provider/runtime milestone. CLI release packaging, three-platform release archives, and final binary self-audit remain Phase 6 work, but GFP establishes the build and test lane they will consume.

## Authoritative Inventory

The inventory is pinned to upstream commit `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`.

- 160 language IDs exist in `CBMLanguage` and `languages.tsv`.
- 159 mappings are available; only `nim` is unavailable.
- The available mappings resolve to 157 unique executable grammars.
- `yaml`, `k8s`, and `kustomize` intentionally share `tree_sitter_yaml` while retaining distinct IDs.
- `objectscript_routine` and `objectscript_udl` are locked asset records with no language binding.
- The lock contains 159 grammar records and 907 assets.
- Exactly 102 grammar records have a root `scanner.c`, and all are C; no C++, Objective-C, or Rust scanner exists in the pinned pack. RST also contains one nested helper named `scanner.c`, so filename counts are not scanner-record counts.
- The full-lock ABI histogram is `{13: 9, 14: 78, 15: 72}`. Excluding the two ABI-15 orphans, the callable-grammar histogram is `{13: 9, 14: 78, 15: 70}`.
- Tree-sitter `0.26.11` accepts the pinned ABI range 13 through 15.

Language-ID normalization is explicit and never inferred by position:

| Language ID | Grammar |
| --- | --- |
| `csharp` | `c_sharp` |
| `dlang` | `d` |
| `emacslisp` | `elisp` |
| `k8s` | `yaml` |
| `kustomize` | `yaml` |
| `llvm_ir` | `llvm` |
| `makefile` | `make` |
| `vimscript` | `vim` |

Factory-symbol exceptions are also explicit and case-sensitive:

| Grammar | Exported symbol |
| --- | --- |
| `assembly` | `tree_sitter_asm` |
| `cobol` | `tree_sitter_COBOL` |
| `gotemplate` | `tree_sitter_gotmpl` |
| `janet` | `tree_sitter_janet_simple` |
| `php` | `tree_sitter_php_only` |
| `protobuf` | `tree_sitter_proto` |
| `qml` | `tree_sitter_qmljs` |
| `sshconfig` | `tree_sitter_ssh_config` |

## Considered Approaches

### 1. Extracted pack verifier plus dedicated native crate — selected

Move lock/materialized-pack integrity into a tool-neutral crate. Compile the full pack in a dedicated native/FFI crate and adapt it to `GrammarProvider` from `goldeneye-syntax`.

This keeps dependency direction acyclic, gives build scripts the same verifier used by `xtask`, confines unsafe code, and preserves fast default builds.

### 2. Build the full pack directly in `goldeneye-syntax`

This uses fewer crates but creates three problems: the build script cannot cleanly reuse the current verifier, the syntax crate must weaken its workspace-wide unsafe policy, and its default core grammar symbols collide with the full-pack symbols.

This approach is rejected.

### 3. Prebuild target-specific native archives in `xtask`

This can shorten Cargo builds, but it creates a second artifact format keyed by platform, compiler, flags, and lock state. Stale artifact handling and release distribution become more complex, while source provenance becomes less direct.

Prebuilt archives may later be used only as exact-key CI caches. They are not source-of-truth or release evidence.

## Crate Boundaries

### `goldeneye-grammar-pack`

This new safe Rust crate owns:

- the `full-pack.toml` schema and validation;
- domain-separated lock and asset hashing;
- directory and exact-Git verification;
- `pack-state.json` parsing and expected-state construction;
- exact materialized-layout verification, including rejection of extra, missing, non-regular, or symlinked assets;
- read-only metadata queries used by generation and native builds.

The existing implementation moves from `goldeneye-syntax`. `goldeneye-syntax` re-exports the existing public pack types so current callers retain source compatibility. `xtask` depends directly on `goldeneye-grammar-pack` and remains responsible for atomic filesystem publication.

### `goldeneye-full-grammars`

This new internal crate owns:

- the opt-in native build script;
- one isolated C translation unit per locked grammar record;
- the generated Tree-sitter factory declarations;
- the sole audited Rust FFI/unsafe boundary;
- a small safe lookup API returning `LanguageFn` plus immutable locked metadata.

The crate does not depend on `goldeneye-syntax`. It depends on `goldeneye-grammar-pack` only as an optional build dependency and on `tree-sitter-language` only in its compiled feature.

The crate deliberately does not inherit the workspace Rust lint table because its `unsafe_code = "forbid"` cannot be lowered. Its manifest repeats the workspace Clippy policy, sets `unsafe_code = "deny"` locally, and places a narrow `#[allow(unsafe_code)]` only on the generated FFI module. Callers cannot access raw extern functions or pointers.

### `goldeneye-syntax`

The syntax crate continues to own `GrammarProvider`, `Grammar`, `GrammarSource`, parser reuse, snapshots, diagnostics, locators, and inspection.

It gains `FullGrammarProvider`, which:

- performs a lexical lookup by `LanguageId`;
- converts the native crate's `LanguageFn` to `tree_sitter::Language`;
- checks the runtime ABI against the locked ABI;
- returns `GrammarSource::FullPack { grammar, source_hash }`;
- returns the existing typed unsupported-grammar error for `nim` and unknown IDs;
- reports exactly 159 supported IDs in lexical order.

## Feature and Link Model

The six maintained core grammar crates export symbols that also occur in the full pack. Cargo features are additive, so feature exclusion alone cannot prevent a future transitive dependency from enabling both implementations.

Every full-pack wrapper therefore renames its language factory and the five standard external-scanner entry points with a `goldeneye_full_` prefix before including the locked sources. Generated Rust declarations link only to those prefixed names. Core crate symbols remain unchanged. A mixed core+full link test is mandatory and catches any unprefixed exported symbol missed by generation.

`goldeneye-syntax` defines two provider features:

```toml
[features]
default = ["core-grammars"]
core-grammars = [
  "dep:tree-sitter-go",
  "dep:tree-sitter-javascript",
  "dep:tree-sitter-python",
  "dep:tree-sitter-rust",
  "dep:tree-sitter-typescript",
]
full-grammar-pack = ["dep:goldeneye-full-grammars"]
```

The full dependency explicitly enables `goldeneye-full-grammars/compiled`. `goldeneye-full-grammars` itself uses `default = []`; its feature-off build script performs no cache access. Both syntax features may be activated safely for the mixed-link sentinel, although production full builds disable syntax defaults so they do not carry redundant core grammars.

The default workspace lane remains:

```text
cargo test --workspace
```

The full lane is explicit:

```text
GOLDENEYE_GRAMMAR_PACK_DIR=<verified-cache> \
  cargo test -p goldeneye-syntax \
  --no-default-features --features full-grammar-pack
```

Consumers added later should disable syntax defaults and propagate their intended provider flavor. The full lane also inspects the Cargo feature graph to prove its artifact did not accidentally include core grammar crates.

## Lock and Generated Registry

Each grammar record gains an `exported_symbol` field. The exporter extracts the direct `TSLanguage` factory from `parser.c`, validates it as one unique C identifier, and cross-checks bound factories against upstream `lang_specs.c`.

Persisting the symbol prevents runtime code from guessing factory names and preserves the `tree_sitter_COBOL` case distinction. The exporter retains its existing streamed hashing and ABI extraction so the 104 MiB parser is never loaded as an unbounded buffer.

`cargo xtask grammars generate-provider` reads only the validated lock and produces deterministic checked-in Rust registry source. Its first line is a strict `// goldeneye-full-pack-lock-sha256: <64 lowercase hex>` header that the native build validates before compiling. Generation contains no timestamps, host paths, or enumeration-order assumptions. `--check` compares generated bytes without mutating the worktree.

`cargo xtask grammars generate-notices` deterministically produces `grammars/full-pack-license-ledger.md`: one row per locked grammar with repository, pinned revision or missing-revision explanation, direct license path, and source hash. Full CI checks this ledger while pack verification authenticates the referenced license bytes.

The generated registry contains:

- 160 lexically keyed language records;
- 159 callable ID rows and one unavailable row for `nim`;
- 157 unique callable grammar/factory records;
- exact factory declarations using ordinal Rust identifiers plus `#[link_name = "goldeneye_full_..."]`, derived from the locked upstream symbol;
- the lock hash, ABI, scanner type, grammar name, and source hash;
- no entry for either ObjectScript orphan.

## Native Build

The native feature requires `GOLDENEYE_GRAMMAR_PACK_DIR`; Cargo does not expose a stable workspace target directory to package build scripts. An unset variable or invalid cache fails before C compilation and prints the exact `cargo xtask grammars sync` remediation.

Before invoking the compiler, the build script verifies:

1. `pack-state.json` matches the current lock hash, commit, grammar count, and asset count;
2. the materialized directory has the exact expected layout;
3. all 907 locked assets still match their domain-separated SHA-256 hashes;
4. every scanner type is either `none` or `c`;
5. the generated registry lock hash matches the lock being compiled.

The build script emits `rerun-if-env-changed` for the cache path and `rerun-if-changed` for the lock, state file, and every locked asset. It performs no Git operation, package download, or HTTP request.

For each of the 159 locked grammar records, the build creates an `OUT_DIR` wrapper. Upstream has 157 application wrappers; GFP deliberately synthesizes two additional compile-only wrappers for the ObjectScript orphan records:

```c
#define tree_sitter_yaml goldeneye_full_tree_sitter_yaml
#define tree_sitter_yaml_external_scanner_create goldeneye_full_tree_sitter_yaml_external_scanner_create
#define tree_sitter_yaml_external_scanner_destroy goldeneye_full_tree_sitter_yaml_external_scanner_destroy
#define tree_sitter_yaml_external_scanner_scan goldeneye_full_tree_sitter_yaml_external_scanner_scan
#define tree_sitter_yaml_external_scanner_serialize goldeneye_full_tree_sitter_yaml_external_scanner_serialize
#define tree_sitter_yaml_external_scanner_deserialize goldeneye_full_tree_sitter_yaml_external_scanner_deserialize
#include "<grammar>/parser.c"
#include "<grammar>/scanner.c" /* only when scanner_language = "c" */
```

The example uses YAML; generation substitutes the exact locked factory symbol and never guesses it from the directory name. Each wrapper is a separate C11 translation unit and a separately named static archive. The verified pack root is an explicit compiler include directory, allowing `"<grammar>/parser.c"` to resolve while nested quoted includes continue to resolve relative to their locked source files. This mirrors the pinned upstream `grammar_*.c` model, which intentionally includes the root parser and scanner in one per-grammar translation unit. It also keeps compiler failures attributable to one grammar. `_DEFAULT_SOURCE` is defined; warnings from generated sources are disabled; MSVC receives UTF-8 and large-object flags when supported.

Only root `parser.c` and root `scanner.c` are wrapper includes. Helper sources remain in their locked hierarchy and are included transitively:

- Crystal includes `unicode.c` from its scanner;
- RST includes its nested scanner, `chars.c`, and helper `parser.c` transitively;
- YAML selects its default core schema; JSON and legacy schema sources remain locked provenance assets;
- path-sensitive FSharp, QML, VHDL, TypeScript-family, HTML-family, and XML headers remain in place.

All 159 records compile. Only the 157 callable factories are referenced by the safe generated registry. Whole-archive linking is prohibited, and the full-pack link audit checks that the two ObjectScript factory symbols are absent from the runtime test binary.

## Runtime and Safety Invariants

- The FFI crate is the only crate allowed to declare or construct a raw Tree-sitter language function.
- All other workspace crates retain `unsafe_code = "forbid"`.
- Generated factory functions are never called directly by application code.
- The provider validates locked ABI versus `Language::abi_version()` before returning a grammar.
- `Parser::set_language` remains the final Tree-sitter compatibility gate.
- Registry lookup is a binary search over static lexical tables; no runtime hash map or full-source metadata allocation is required.
- YAML-family IDs share one language function but retain distinct `LanguageId` values in snapshots and graph identities.
- Core and full providers can coexist without native symbol collision; official full artifacts still exclude core features to avoid redundant code.
- Builds never infer mappings by enum position, grammar-directory spelling, or factory-name spelling.

## Verification

### Fast/default lane

- `cargo fmt --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- Python exporter unit tests and deterministic provider-generation checks
- deterministic 159-entry license-ledger generation checks
- a cache-free build proving the native feature is not activated by default

### Full-pack lane

- reproduce `full-pack.toml` from the pinned upstream commit;
- verify and atomically materialize the exact Git blobs;
- rerun directory verification on the cache;
- generate the provider with `--check`;
- build the native crate with Cargo offline;
- compile all 159 wrappers;
- link all 157 callable grammar objects;
- run a mixed core+full link sentinel and inspect the full-only Cargo feature graph for accidental core grammar crates;
- iterate all 159 supported IDs, validate metadata and ABI, call `Parser::set_language`, and parse an empty buffer;
- assert the `160/159/157/2` cardinalities and both normalization tables;
- assert `nim` is unavailable and both ObjectScript records are unreachable;
- inspect the linked test binary or linker map and assert both ObjectScript factory symbols are absent;
- assert YAML/K8s/Kustomize share one language while retaining distinct IDs;
- run non-empty parse fixtures for Crystal, RST, YAML, VHDL, FSharp, QML, PureScript, and ReScript;
- run concurrent lookup coverage proving `FullGrammarProvider: Send + Sync`;
- rerun the default workspace lane after the full build to detect feature or cache leakage.

Empty/all-ID probes prove factory, link, ABI, parser acceptance, and basic lifecycle compatibility; they do not claim behavioral conformance for every scanner token path. The targeted non-empty fixtures are scanner regressions, while broad grammar/extraction parity belongs to the following index/extraction phase.

Full-pack CI also reproduces a checked-in 159-entry license ledger from the lock, verifies every locked direct license asset through normal pack hashing, and checks the existing notice documentation. Phase 6 packages the full license texts, but notice/ledger reproducibility is GFP evidence.

The initial CI full-pack job runs on Linux after explicit network acquisition and `cargo fetch`. It sets `CARGO_NET_OFFLINE=true` for generation/build/tests. Windows/MSVC and macOS full release gates are added in Phase 6; local GFP verification on the implementation host must still prove MSVC compilation.

## Documentation and Claims

Documentation distinguishes:

- `core-only`: the default development provider and normal CI lane;
- `full-grammar-pack`: the source-built provider proven by the full-pack lane.

The full provider is not advertised as a publishable release artifact until Phase 6 also bundles all 159 license texts, records compiler/platform evidence, and runs the final binary self-test. GFP may claim runtime provider completeness, not final delivery completeness.

## Non-Goals

- Porting graph extraction, SQLite storage, ACK query tools, or structural edits in this phase.
- Dynamically loading grammar libraries at runtime.
- Downloading grammars from a Cargo build script.
- Treating prebuilt native archives as authoritative inputs.
- Making `nim` callable without a pinned upstream grammar.
- Exposing the two ObjectScript orphan assets through a public lookup escape hatch.
- Linking upstream application C code or its bundled Tree-sitter runtime.

## Completion Gate

GFP is accepted only after task-level TDD evidence, independent spec and code-quality reviews, a fresh goal-level integration review, clean default and full-pack gates, deterministic regeneration, and a clean worktree. Core-only success is not GFP completion evidence.
