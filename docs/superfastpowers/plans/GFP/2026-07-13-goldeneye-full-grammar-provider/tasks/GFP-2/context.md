# GFP-2 Context

- Status: pending
- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan baseline commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Reviewed commit/range: none

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

Pending. No implementation, review, commit-range, or verification evidence exists yet.

## First Action

Wait for GFP-1, then start streamed factory-extraction tests in Step 1.
