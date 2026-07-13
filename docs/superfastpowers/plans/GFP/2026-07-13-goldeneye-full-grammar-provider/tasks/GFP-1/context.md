# GFP-1 Context

- Status: implemented; specification and code-quality reviews pending
- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan baseline commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Implementation commit: `514f41a`
- Reviewed commit/range: pending

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

- RED: `cargo test -p goldeneye-syntax --test grammar_lock` exited 101 with unresolved import `goldeneye_grammar_pack` from the compatibility type-identity test.
- RED: `cargo test -p goldeneye-grammar-pack --test materialized_pack` exited 101 because the new workspace crate had no manifest yet.
- GREEN: materialized-pack tests passed `11/11`; syntax grammar-lock tests passed `8/8`; xtask grammar-sync tests passed `15/15`.
- Focused Clippy initially rejected a used underscore-prefixed fixture field. Systematic debugging traced the lint to symlink tests accessing a field named `_temporary`; renaming it to `temporary` made the unchanged focused Clippy gate pass.
- Final gates passed: `cargo fmt --check`; focused Clippy for `goldeneye-grammar-pack`, `goldeneye-syntax`, and `xtask`; `cargo test -p goldeneye-grammar-pack`; syntax grammar-lock tests; xtask grammar-sync tests; `cargo test --workspace`; and `git diff --check`.
- Dependency audit confirmed `goldeneye-grammar-pack` has no Tree-sitter, MCP, syntax-engine, atomic-publication, or replacement dependency; `xtask` now depends on the pack crate directly.
- Implementation commit: `514f41a` (`[GFP-1] refactor: extract grammar pack integrity crate`).
- Specification review and code-quality review remain pending; their review files were not modified by the implementer.

## First Action

Review implementation commit `514f41a` for GFP-1 specification compliance and code quality.
