# GFP-1 Specification Review

- Result: approved
- Reviewer: Codex specification review
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Baseline: plan `6e2b800`; design whitespace follow-up `023837d`
- Reviewed range: `514f41a^..514f41a` (`[GFP-1] refactor: extract grammar pack integrity crate`)

## Review Scope

Verify the pack implementation is moved once into `goldeneye-grammar-pack`, exact materialized-state verification is shared, syntax re-exports remain source-compatible, and xtask alone owns state writing/atomic publication.

## Constraints to Check

All existing validation, streamed hashing/copying, exact-Git, license, count, and domain-hash rules must remain; the new crate must stay safe and runtime-independent; the default lane must remain cache-free.

## Required Gates

Inspect the actual GFP-1 commit/range and changed files, then validate the focused pack/syntax/xtask tests, workspace tests, formatting, Clippy, and diff check recorded in `context.md`.

## Evidence

- [x] Scope and extraction: the reviewed range changes only the GFP-1 implementation paths. Git identifies `pack/git_source.rs` as a 99% move and `pack.rs` as an 86% move into `goldeneye-grammar-pack`; the old syntax files are removed, all prior production function names remain in the extracted crate, and the shared state/layout verifier definitions occur only there.
- [x] Dependency boundary: `goldeneye-grammar-pack` has only `cap-primitives`, Serde/JSON, SHA-2, `thiserror`, and TOML as production dependencies (`tempfile` is test-only). It adds no Tree-sitter, MCP, syntax-engine, native, or atomic-publication dependency.
- [x] Expected state and verification order: `GrammarPackState` retains the five-field schema with unknown-field rejection. `GrammarPackState::expected` computes schema version, lock-file hash, upstream commit, grammar count, and locked-asset count. `verify_materialized_pack` opens a regular state file, compares expected state, verifies the exact layout, then delegates to streamed source/hash verification before returning `VerifiedPack`.
- [x] Exact layout and symlink coverage: the verifier derives the complete expected file and directory sets, rejects extra/missing/non-regular entries, and rejects symlink/reparse entries. The 11 focused fixtures cover exact success, lock mismatch, invalid/unknown state data, missing assets, extra files/directories, final and intermediate symlinks, same-size hash drift, and repeat verification without mutation.
- [x] Syntax identity: `goldeneye-syntax` publicly re-exports the pack-crate types and functions directly. The compile-time compatibility test passes a `goldeneye_grammar_pack::GrammarPackLock` to a function accepting `goldeneye_syntax::GrammarPackLock`, proving type identity rather than a compatibility copy.
- [x] xtask ownership and reuse: `xtask` imports `GrammarPackState::expected`, `PACK_STATE_FILE`, and `verify_materialized_pack` from the shared crate. It alone writes state, manages owned sibling temporary directories, verifies the completed temporary pack, and performs no-replace atomic publication and owned cleanup; no duplicate private state/layout verifier remains.
- [x] Gate evidence reviewed from `implementer-handoff.md`: RED failures were recorded before implementation; GREEN results were pack materialized `11/11`, syntax grammar-lock `8/8`, and xtask grammar-sync `15/15`. The handoff also records fresh formatting, focused Clippy, all pack tests, focused syntax/xtask tests, workspace tests, and `git diff --check` as passing. This specification review used focused read-only inspection and did not rerun the full workspace.

## Findings

No specification findings. GFP-1 is approved for code-quality review.
