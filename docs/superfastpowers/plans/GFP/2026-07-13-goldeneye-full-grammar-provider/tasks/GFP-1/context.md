# GFP-1 Context

- Status: implementation and both requested code-quality repairs complete; specification approved; code-quality re-review pending
- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan baseline commit: `6e2b800`
- Design whitespace follow-up: `023837d`
- Implementation commit: `514f41a`
- Specification review commit: `26ab716` (`approved`)
- Code-quality review commit: `27e28f7` (`changes requested`)
- Repair commit: `7fa41c1`
- Code-quality re-review commit: `83a77d8` (`changes requested`)
- Junction repair commit: `5bddeea`

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
- GREEN: materialized-pack tests passed `13/13`; syntax grammar-lock tests passed `8/8`; xtask grammar-sync tests passed `16/16` after both repair regressions were added.
- Focused Clippy initially rejected a used underscore-prefixed fixture field. Systematic debugging traced the lint to symlink tests accessing a field named `_temporary`; renaming it to `temporary` made the unchanged focused Clippy gate pass.
- Repair RED/GREEN: the existing-pack corruption regression first failed to compile with `E0599` because `XtaskError::ExistingPack` did not exist, then passed after the new variant preserved `PackError::HashMismatch` as its `#[source]` while retaining the `existing destination` display context.
- Repair RED/GREEN: forcing link creation to fail made both link/reparse tests fail instead of silently returning. Native Windows symlink creation then reproduced OS error 1314; the platform-safe fixture now falls back to unprivileged directory junctions, asserts the reparse-point attribute, and passes both focused tests with real reparse fixtures.
- Junction repair RED/GREEN: a valid `junction&probe` path first failed because `cmd.exe` parsed `&` as control syntax and attempted to execute `probe`; the same focused test passed after replacing `cmd /C mklink /J` with `junction::create`.
- Junction cleanup RED/GREEN: an overlong target reproduced `junction` creating `partial-junction` before returning `InvalidInput`; the helper now best-effort removes only a link path proven absent before the call, and the regression asserts both the path and temporary directory are clean.
- The Windows-only dev dependency is pinned to `junction = 2.0.0` with default features disabled; the feature tree confirms `unstable_admin` is not enabled, while native symlink-first and OS-error-1314 fallback behavior remain unchanged.
- Final gates passed: `cargo fmt --check`; focused Clippy for `goldeneye-grammar-pack`, `goldeneye-syntax`, and `xtask`; `cargo test -p goldeneye-grammar-pack`; syntax grammar-lock tests; xtask grammar-sync tests; `cargo test --workspace`; and `git diff --check`.
- Dependency audit confirmed `goldeneye-grammar-pack` has no Tree-sitter, MCP, syntax-engine, atomic-publication, or replacement dependency; `xtask` now depends on the pack crate directly.
- Implementation commit: `514f41a` (`[GFP-1] refactor: extract grammar pack integrity crate`).
- Repair commit: `7fa41c1` (`[GFP-1] fix: preserve pack errors and enforce reparse tests`).
- Junction repair commit: `5bddeea` (`[GFP-1] fix: remove shell junction fallback`).
- The code-quality review remains `changes requested` until a reviewer updates it; specification and code-quality review files were not modified by the repair implementer.

## First Action

Re-review implementation commit `514f41a` plus repair commits `7fa41c1` and `5bddeea` for GFP-1 code quality.
