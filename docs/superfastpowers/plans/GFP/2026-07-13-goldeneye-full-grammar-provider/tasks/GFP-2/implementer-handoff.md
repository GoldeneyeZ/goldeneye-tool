# GFP-2 Implementer Handoff

- Status: implementation complete; awaiting formal review
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Implementation baseline: `af5504a`
- Implementation commit: `95f596e`
- Review range: `af5504a..95f596e`

## Scope

Implemented exact `exported_symbol` extraction and validation, reproduced the pinned lock, generated the safe prefixed registry and deterministic license ledger, and established the default-empty native crate.

## Delivered Contract

- Exporter parsing and hashing are streamed, including guarded 104 MiB input; direct definitions are boundary-safe and duplicates, malformed symbols, swapped factories, and unsupported scanner languages are rejected.
- The real lock contains all 159 unique exported symbols, including all eight normalization exceptions, all eight factory exceptions, and case-sensitive `tree_sitter_COBOL`.
- The provider is lexical and order-independent: 160 language rows, 159 available IDs, 157 callable grammars/factories, typed-unavailable Nim, YAML aliases, and no ObjectScript runtime row.
- Generated Rust embeds the domain-separated exact lock-byte hash, exact upstream commit, ABI/scanner/source metadata, safely escaped strings, and 157 ordinal link symbols. Unsafe declarations are confined to the generated module, which the default build does not load.
- Provider and notice generation use adjacent atomic temporary files; `--check` performs no writes for missing, stale, unchanged, or read-only outputs.
- The license ledger has 159 lexical rows with repository, revision or explicit reason, direct license, and source hash, with no timestamp, host path, or upstream-order dependence.

## TDD and Adversarial Evidence

- Exporter tests were written first and observed RED against the missing factory-symbol/exported-field contract, then reached 19/19 GREEN.
- Provider-generation tests were written before the renderer/commands/crate existed and observed RED, then reached 7/7 GREEN.
- A deliberate one-position grammar-index shift failed on the exact Ada mapping; reversing `FACTORIES` failed the ordinal assertion. Both mutations were reverted before the final replay.
- Tests parse generated Rust with `syn`, assert every one of the 160 language rows and 157 extern/factory positions, verify both ABI histograms, exercise adversarial Rust/Markdown escaping, and prove non-mutating check mode and CLI wiring.

## Final Verification

- `python tools/test_export_grammar_lock.py` — 19 passed.
- Pinned exporter `--check` — `grammars/full-pack.toml` reproducible.
- Provider `--check` and root license-ledger `--check` — current.
- Grammar-pack tests — 13 passed; syntax grammar-lock — 14 passed; grammar sync — 16 passed; provider generation — 7 passed.
- `cargo check -p goldeneye-full-grammars` and `--no-default-features` — passed.
- `cargo test --workspace` — passed with zero failures.
- `cargo clippy --workspace --all-targets -- -D warnings` — passed.
- `cargo fmt --all -- --check`, `git diff --check`, and staged diff check — passed.

## Compatibility and Review Notes

- `schema_version` intentionally remains 1. GFP-2 is an atomic pre-release v1 schema evolution; old readers that predate required `exported_symbol` are not promised compatibility. The exporter, Rust reader, fixtures, and generated artifacts advance together.
- `syn` is an xtask dev dependency solely to syntax-validate generated Rust. Existing direct `GrammarRecord` fixtures were updated because `exported_symbol` is now required.
- The exact-command Python harness adds the repository root to `sys.path`; this makes the plan's `python tools/test_export_grammar_lock.py` invocation work without changing production import behavior.
- ACK's ready graph lacked the new uncommitted symbols, so implementation discovery fell back through Context Mode after weak graph results.
- Out-of-scope follow-up: the two pre-existing `goldeneye-syntax` direct symlink tests silently return when fixture creation fails. Mandatory materialized-pack link tests are green, but direct `verify_source` assertions should move into mandatory pack tests separately.

## Formal Review Request

Review `af5504a..95f596e`. An independent implementer-side checkpoint found no Critical or Important issues and assessed the range ready to merge. No formal spec-review or code-quality verdict has been recorded yet; reviewers own `spec-review.md` and `code-quality.md`.
