# GFP-4 Context

Status: Implemented in `2a27273`. Per the active plan-progression bypass, GFP-4 did not run task-level spec or code-quality reviews; its evidence is queued for the single goal-level audit.

Authoritative inputs:

- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Starting baseline: `62dfd35`
- Implementation commit: `2a27273`
- Inherited full-pack lock SHA-256: `ce668d1c07d4f7dd72fd8f167f94d218bfc933a1ccd9ffa52277354968c950c1`

Implemented outcome:

- `goldeneye-syntax` now defaults to explicit feature `core-grammars`; its five maintained grammar dependencies are optional and enabled by that feature. Existing default behavior and provenance are unchanged.
- Opt-in `full-grammar-pack` enables `goldeneye-full-grammars/compiled` without enabling the maintained core grammar crates. Core and full features may also be enabled together.
- `FullGrammarProvider` uses only the safe compiled registry. It maps `LanguageFn` into `tree_sitter::Language`, checked-converts the runtime ABI, rejects drift through typed `GrammarAbiMismatch`, preserves the requested `LanguageId`, and returns locked grammar name/source-hash provenance.
- `supported_ids` maps the generated lexical 159-ID iterator directly into `LanguageId` values. It does not rebuild or sort mappings at runtime.
- The public surface exposes no raw factory function or FFI pointer. Unsafe code remains private to GFP-3's generated native module.
- Existing core-dependent integration suites are feature-gated, so the full-only lane links none of the five core grammar crates; all-features still runs both suites as the collision sentinel.

Runtime audit:

- Exact shape: 160 declared IDs, 159 supported/available IDs, one unavailable ID (`nim`), 157 unique grammars, and two ObjectScript orphan sources absent from every runtime query.
- Exact language-to-grammar exception table has eight rows: `csharp/c_sharp`, `dlang/d`, `emacslisp/elisp`, `k8s/yaml`, `kustomize/yaml`, `llvm_ir/llvm`, `makefile/make`, and `vimscript/vim`.
- Exact grammar-to-factory exception table has eight rows: assembly, COBOL, Go template, Janet, PHP, Protobuf, QML, and SSH config.
- Every supported ID loads through the provider, returns exact locked metadata, reports ABI 13 through 15 matching both the lock and runtime, passes `Parser::set_language`, parses empty input, and survives normal creation/drop lifecycle.
- YAML, K8s, and Kustomize retain three IDs with equal Tree-sitter languages. Non-empty Crystal, RST, YAML, VHDL, FSharp, QML, PureScript, and ReScript fixtures parse without errors.
- Concurrent lookups pass and `FullGrammarProvider` is `Send + Sync`. The all-features test switches one parser among core Rust and full Rust/YAML/K8s/Kustomize values in one binary.

TDD and gate evidence:

- RED: `cargo test -p goldeneye-syntax --test full_grammars --no-default-features --features full-grammar-pack` failed because `goldeneye-syntax` did not contain feature `full-grammar-pack`.
- Focused GREEN with verified cache/offline Cargo: 5/5 full-provider integration tests passed.
- Full-only package tests passed, including 5/5 provider tests and 12/12 lock tests. Full-only Clippy passed with `-D warnings`.
- `cargo test -p goldeneye-syntax --all-features` passed all suites; the dedicated mixed core/full test linked and ran successfully.
- `cargo tree -p goldeneye-syntax --no-default-features --features full-grammar-pack -e features` contained `goldeneye-full-grammars/compiled` and contained none of `tree-sitter-go`, `tree-sitter-javascript`, `tree-sitter-python`, `tree-sitter-rust`, or `tree-sitter-typescript`.
- With `GOLDENEYE_GRAMMAR_PACK_DIR` and `CARGO_NET_OFFLINE` removed, formatting, workspace Clippy, all 39 workspace test suites, release workspace build, and `git diff --check` passed.
- Default-lane cache proof: `target/goldeneye-grammars/pack-state.json` retained SHA-256 `09848ab929b98c01a2f3664a98db734776697cc6f6cceb7a130613befe05b330` and UTC timestamp ticks `639195652280803471` across all default gates.

Debugging record and deviations:

- Full-only Clippy found one unused inferred `Language` test import; removing only that import made the unchanged gate pass.
- The first mixed run exposed that the existing core metadata guard expected bare TOML dependency strings. Optional core dependencies necessarily use inline tables, so the guard now validates exact `version`, requires `optional = true`, and preserves the table when injecting synthetic drift. The mixed rerun passed.
- No plan deviation or blocker remains. GFP-3's accepted seven-file native-support group, 914-asset cache, and bounded COBOL compatibility behavior were consumed unchanged and were not reopened.
