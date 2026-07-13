# GFP-3 Context

Status: Implemented in `18eec00`. Per the current plan-progression bypass, GFP-3 did not run task-level spec or code-quality reviews; its implementation and the exceptions below are queued for the single goal-level audit.

Authoritative inputs:

- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Implementation commit: `18eec00`
- Full-pack upstream commit: `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`
- Full-pack lock SHA-256: `ce668d1c07d4f7dd72fd8f167f94d218bfc933a1ccd9ffa52277354968c950c1`

Implemented outcome:

- `goldeneye-full-grammars` now has an opt-in `compiled` feature. Its default lane has no native build dependencies and does not inspect `GOLDENEYE_GRAMMAR_PACK_DIR`.
- Compiled mode requires a materialized cache, verifies the lock hash, state, layout, asset set, asset hashes, generated provider header, production cardinalities, and supported scanner plan before writing wrappers or invoking a compiler.
- The build emits one deterministic wrapper and archive per compiled source, namespaces each factory plus all five supported scanner exports, includes helper files from their verified locations, and uses no whole-archive flags.
- Unsafe FFI is confined to the feature-gated generated module. The public registry exposes copied metadata and `LanguageFn` values through sorted static slices and binary-search lookup.
- Production counts are 160 declared language IDs, 159 available records/wrappers, 157 unique runtime factories, two ObjectScript orphans, 102 scanners, 57 parser-only grammars, one native-support group, and 914 verified assets.
- Registry tests invoke all 157 factories, check ABI compatibility, link maintained core and full registries together, validate Crystal/RST/YAML/VHDL/FSharp/QML parsing, verify YAML scanner aliases, and confirm ObjectScript symbols are absent from the test binary.

TDD and verification evidence:

- RED began with `cargo test -p goldeneye-full-grammars --test compiled_registry`, which failed because the native build/registry did not exist. Subsequent focused REDs covered the missing native-support schema/export, notices, CFML include layout, COBOL compatibility guard, and MSVC `restrict` handling before each implementation increment.
- `cargo xtask grammars sync --lock grammars/full-pack.toml --git-repo .upstream/codebase-memory-mcp --git-prefix internal/cbm/vendored/grammars --dest target/goldeneye-grammars` passed from a cleared cache and materialized the exact 159-grammar/914-asset pack.
- `GOLDENEYE_GRAMMAR_PACK_DIR=target/goldeneye-grammars CARGO_NET_OFFLINE=true cargo test -p goldeneye-full-grammars --features compiled` passed fresh: 20/20 integration tests.
- Compiled mode without `GOLDENEYE_GRAMMAR_PACK_DIR` failed as expected with exit 101 and the exact sync remediation; default `cargo check -p goldeneye-full-grammars` passed without the variable.
- `cargo test -p goldeneye-grammar-pack` passed 14/14 integration tests; `python tools/test_export_grammar_lock.py` passed 20/20 tests; all xtask suites passed.
- Lock, generated provider, and NOTICE `--check` commands reported current/reproducible outputs.
- `cargo clippy -p goldeneye-full-grammars --all-targets --features compiled -- -D warnings`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` passed.
- Final `cargo fmt --all -- --check` and `git diff --check` passed.

Goal-level audit exceptions:

- The original lock covered 907 grammar-owned assets, but the locked CFML scanner includes `../../common/scanner.h`. The authoritative pack was therefore extended with a separately hashed, separately noticed seven-file `common` native-support group from the same pinned upstream commit. Grammar cardinalities are unchanged; total verified assets are now 914.
- Visual Studio 2017 rejects the verified COBOL scanner's two variable-length local arrays. On MSVC only, `build.rs` accepts exactly scanner SHA-256 `0e146beb0331e4f95e2fb815e263c649f2bc404b35dd1b19eb125cbd4ed95df8`, checks the helper signature/call shape, constant value, declaration text, and occurrence counts, then changes only the two bounds from `[number_of_words]` to `[9]` in an `OUT_DIR`-derived source. Wrong hashes or structural drift fail closed. Non-MSVC builds include the original verified scanner directly; the cache and lock sources are never modified.
- MSVC compiler flags are capability-probed. Unsupported `/std:c11` is omitted on Visual Studio 2017 while verified wrapper-level compatibility supplies `restrict` as `__restrict`; supported `/utf-8` and `/bigobj` flags remain target-aware.
