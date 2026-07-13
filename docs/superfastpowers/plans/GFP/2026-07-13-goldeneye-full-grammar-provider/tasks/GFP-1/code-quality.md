# GFP-1 Code-Quality Review

- Result: changes requested
- Reviewer: Codex code-quality review
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Baseline: plan `6e2b800`; design whitespace follow-up `023837d`
- Reviewed range: `514f41a^..514f41a` (`[GFP-1] refactor: extract grammar pack integrity crate`), after specification approval `26ab716`

## Review Scope

Review the extracted crate boundary, dependency direction, pack-state parsing/layout traversal, streamed I/O, error contracts, symlink/path safety, and removal of duplicate pack logic.

## Constraints to Check

The pack crate must remain safe and read-only, xtask must retain mutation ownership, public syntax types must not fork, and default builds must not touch the grammar cache.

## Required Gates

Review the actual GFP-1 diff/range and fresh formatting, focused Clippy/tests, workspace tests, and diff-check evidence.

## Evidence

- [x] Crate and dependency direction are cohesive: `goldeneye-grammar-pack` owns the shared integrity model, `goldeneye-syntax` re-exports the exact types, and `xtask` depends directly on the pack crate without a syntax/runtime dependency.
- [x] The pack-state model is private-by-construction, rejects unknown JSON fields, computes its five values from the lock, and exposes only the lock hash needed by `xtask`.
- [x] The static-path verifier opens the state and locked assets without following symlinks, rejects reparse/symlink/non-regular entries during exact-layout traversal, hashes from one opened asset handle, and does not mutate a valid materialized cache.
- [x] State writing, temporary-directory ownership, cleanup, and no-replace atomic publication remain in `xtask`; the extracted crate does not own publication.
- [x] The reviewed production move retains one lock/Git/hash implementation. The new exact materialized verifier replaces, rather than duplicates, the former private `xtask` state/layout verifier.
- [x] Implementer evidence records passing formatting, focused Clippy, focused tests, workspace tests, and `git diff --check`. This review reran `cargo test -p goldeneye-grammar-pack --test materialized_pack`: 11 passed, 0 failed.

## Findings

1. **Medium — existing-pack verification erases structured error contracts.** `xtask/src/lib.rs:283`-`289` maps every `PackError` returned by `verify_materialized_pack` to `XtaskError::Invalid` containing only formatted text. Before the extraction, only state-file parse/mismatch failures were classified as invalid; layout I/O remained `XtaskError::Io`, and asset/hash failures remained `XtaskError::Pack`. Callers can no longer distinguish corruption from permissions or I/O, and the original error is no longer available through the source chain. Preserve `PackError`/I/O as a source (either with `?` or a contextual `XtaskError` variant) and reserve `Invalid` for the state/layout conditions intentionally classified that way.
2. **Medium — required symlink coverage can report green without executing.** `crates/goldeneye-grammar-pack/tests/materialized_pack.rs:133`-`135` and `:146`-`148` return from the test when symlink creation fails. On Windows hosts without symlink privileges, both security regressions therefore count as passed rather than skipped or failed, leaving the Windows reparse-point path untested while the suite reports 11/11. Provide a Windows-capable reparse fixture or fail with a clear prerequisite; if a platform truly cannot create the fixture, gate the test explicitly so CI cannot mistake absence of coverage for a pass.
