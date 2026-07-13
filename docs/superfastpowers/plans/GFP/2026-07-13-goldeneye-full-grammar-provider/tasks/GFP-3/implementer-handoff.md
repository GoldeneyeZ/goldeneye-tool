# GFP-3 Implementer Handoff

Status: Implementation complete in `18eec00`; ready for the plan-progression goal-level audit. Task-level spec and code-quality reviews were intentionally bypassed by the current plan instructions.

References: plan `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`, design `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`, plan commit `6e2b800`, design whitespace follow-up `023837d`, implementation commit `18eec00`.

Delivered:

- Opt-in deterministic native compilation for the verified full grammar pack, with cache-independent default builds.
- Exact pre-compiler cache verification, namespaced per-grammar wrappers/archives, five scanner aliases, safe static lookup, confined generated FFI, and no whole-archive/runtime-map mechanism.
- 159 compiled sources and available records, 157 callable unique factories, 102 scanners, 57 parser-only grammars, two unavailable ObjectScript records, one native-support group, and 914 verified assets.
- Domain-separated native-support locking/materialization/notices for the seven shared `common` assets required by CFML.
- Target-aware compiler selection and flags, including the fail-closed MSVC-only COBOL compatibility derivation described below.

Audit hotspots:

- `crates/goldeneye-full-grammars/build.rs`: verification ordering, deterministic wrapper generation, symbol aliases, compiler family checks, layout handling, and the two bounded MSVC compatibility paths.
- `crates/goldeneye-full-grammars/src/lib.rs` and `tests/compiled_registry.rs`: unsafe boundary, copied registry values, lookup semantics, all-factory invocation, link coexistence, target fixtures, and ObjectScript absence.
- `crates/goldeneye-grammar-pack/src/lib.rs`, `tools/export_grammar_lock.py`, and `grammars/full-pack.toml`: the separately hashed `native_support` model and exact Git/directory materialization.
- `xtask/src/lib.rs` and `NOTICE`: deterministic support provenance and license rendering.

Required exception review:

- `common` support expansion: seven assets from the same pinned codebase-memory upstream commit were added because CFML's verified scanner reaches outside its grammar directory. This changes assets from 907 to 914 but changes no grammar, wrapper, scanner, or runtime-factory cardinality.
- COBOL on MSVC: only when `_MSC_VER` is active, a verified scanner with exact SHA-256 `0e146beb0331e4f95e2fb815e263c649f2bc404b35dd1b19eb125cbd4ed95df8` is structurally checked and copied to `OUT_DIR` with exactly two VLA bounds changed to literal `9`. Any hash, signature, call-count, constant, or declaration drift aborts before compilation. All non-MSVC targets compile the original verified source directly.
- Visual Studio 2017: `/std:c11` is used only if the compiler accepts it; the wrapper supplies the MSVC spelling for `restrict`. `/utf-8` and `/bigobj` are likewise probe-driven.

Recorded gates:

- RED: focused registry test initially failed on the absent build script/API; narrower REDs then covered each support/layout/MSVC behavior.
- Fresh verified cache sync: passed at upstream `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`.
- Final compiled gate: `GOLDENEYE_GRAMMAR_PACK_DIR=target/goldeneye-grammars CARGO_NET_OFFLINE=true cargo test -p goldeneye-full-grammars --features compiled` passed 20/20.
- Negative paths: missing environment/cache remediation, stale/extra/hash-drifted assets, stale generated header, unsupported scanners, wrong COBOL hash, and COBOL structural drift all fail before wrapper/compiler side effects.
- Reproducibility: exporter, provider, and notices `--check` gates passed at lock SHA-256 `ce668d1c07d4f7dd72fd8f167f94d218bfc933a1ccd9ffa52277354968c950c1`.
- Quality: exporter 20/20, grammar-pack 14/14, all xtask suites, compiled Clippy, workspace Clippy, workspace tests, formatting, and diff checks passed.

No implementation work remains in GFP-3. The next authorized action is the single goal-level audit defined by plan progression; do not infer task-level review artifacts from this handoff.
