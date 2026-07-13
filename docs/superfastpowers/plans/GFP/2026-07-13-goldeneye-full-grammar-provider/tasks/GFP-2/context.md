# GFP-2 Context

- Status: implementation complete; awaiting spec and code-quality review
- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan baseline commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Implementation baseline: `af5504a`
- Implementation commit: `95f596e`
- Recommended review range: `af5504a..95f596e`
- Formal reviewed commit/range: none

## Scope

Persist exact exported factory symbols in the real lock, validate/cross-check them, generate the deterministic full-provider registry and 159-entry license ledger, and create the default-empty full-grammar crate.

## Constraints

- Factory extraction and hashing must remain streamed, including the 104 MiB parser.
- Preserve `160/159/157/2`, existing asset hashes/counts, ABI histograms, COBOL case, exception tables, and orphan exclusion.
- Generated files must be byte-stable with safe escaping and no timestamps, host paths, or upstream-order assumptions.
- The new full-grammar crate must compile by default without a cache.

## Required Gates

Python exporter tests and real-lock `--check`; provider and notice generator `--check`; pack, grammar-lock, and provider-generation tests; default-empty crate check; workspace tests; and `git diff --check`.

## Evidence

- The regenerated lock has 159 grammar records, 159 unique exported symbols, 160 language mappings, 907 unchanged assets, 148 pinned revisions, and 11 explicit missing-revision reasons. Its diff is exactly 159 `exported_symbol` additions and no record or asset removal.
- The generated provider has 157 ordinal extern declarations, 157 factory entries, 157 callable grammar records, and 160 lexical language rows: 159 available plus typed-unavailable Nim. YAML, Kustomize, and K8s share YAML; ObjectScript remains excluded from runtime mappings.
- Full ABI counts remain `{13: 9, 14: 78, 15: 72}`; callable counts are `{13: 9, 14: 78, 15: 70}`. The license ledger has exactly 159 deterministic rows.
- `python tools/test_export_grammar_lock.py`: 19 passed.
- Real upstream exporter `--check`: reproducible at `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`.
- Provider and notice generator `--check`: current.
- `cargo test -p goldeneye-grammar-pack`: 13 passed; syntax grammar-lock: 14 passed; provider generation: 7 passed; grammar sync: 16 passed.
- Default-empty full-grammar crate check, workspace tests, workspace Clippy with `-D warnings`, Rust formatting, and `git diff --check`: passed.
- Deliberate registry-index and factory-order mutations each failed the exactness tests, then were reverted before the final verification replay.
- An independent implementer-side review of `af5504a..95f596e` found no Critical or Important issues and assessed the range ready to merge; formal spec and code-quality reviews remain pending.

The lock remains `schema_version = 1` by controller decision. This is an atomic pre-release v1 evolution: the exporter, Rust reader, generated outputs, and fixtures move together, with no compatibility promise for an older v1 reader that predates `exported_symbol`. `load_with_hash` hashes the exact byte buffer it parses.

ACK reported a ready index but did not yet contain the new uncommitted generator symbols, so weak graph results were followed by Context Mode exploration as permitted by the project routing rules.

One pre-existing follow-up remains outside GFP-2: the two direct `verify_source` symlink tests in `goldeneye-syntax` can return early when link-fixture creation fails. Mandatory materialized-pack symlink coverage passes, but direct-API coverage should be moved into mandatory pack tests in a separate cleanup.

## Reviewer First Action

Review `af5504a..95f596e`, concentrating on streamed extraction boundaries, exact mapping/factory joins, deterministic escaping and atomic check mode, and default-empty/lint-isolated crate behavior.
