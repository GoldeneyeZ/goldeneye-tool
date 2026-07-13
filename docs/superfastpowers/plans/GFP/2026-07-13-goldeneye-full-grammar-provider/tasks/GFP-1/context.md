# GFP-1 Context

- Status: pending
- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan baseline commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Reviewed commit/range: none

## Scope

Extract lock/Git verification into the safe `goldeneye-grammar-pack` crate, move exact materialized-pack verification down from `xtask`, preserve `goldeneye-syntax` public re-exports, and keep atomic publication in `xtask`.

## Constraints

- Move the implementation rather than duplicate it or change behavior.
- Preserve path, license, streamed hashing, exact-Git, count, and hash-domain invariants.
- Add no Tree-sitter, MCP, syntax-engine, or filesystem-mutation dependency to the pack crate.
- Default workspace commands must not require a grammar cache.

## Required Gates

`cargo fmt --check`; focused Clippy for the pack/syntax/xtask crates; pack, grammar-lock, and grammar-sync tests; `cargo test --workspace`; and `git diff --check`.

## Evidence

Pending. No implementation, review, commit-range, or verification evidence exists yet.

## First Action

Start Step 1 with failing crate-boundary and materialized-state tests.
