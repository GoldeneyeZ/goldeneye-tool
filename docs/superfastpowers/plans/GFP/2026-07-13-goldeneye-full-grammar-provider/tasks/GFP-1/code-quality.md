# GFP-1 Code-Quality Review

- Result: changes requested
- Reviewer: Codex code-quality review
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Baseline: plan `6e2b800`; design whitespace follow-up `023837d`
- Reviewed ranges: implementation `514f41a^..514f41a`; repair re-review `27e28f7..64d8d6b` (code repair `7fa41c1`, repair record `64d8d6b`), after specification approval `26ab716`

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
- [x] Original structured-error finding resolved: `XtaskError::ExistingPack` retains `PackError` as its typed `#[source]` and adds existing-destination context. The regression test asserts the outer variant, `PackError::HashMismatch`, display context, and downcast source chain. A focused re-run passed 1/1.
- [x] Original silent-skip finding resolved: both link/reparse tests now require fixture creation and assert the final path has symlink/reparse metadata. Captured mutation runs show forced fixture failure made each test fail, so the RED evidence is truthful.
- [x] On the normal Windows temp path, native error 1314 exercised the junction fallback and both focused reparse tests passed 2/2 with their metadata assertions. The recorded post-repair formatting, Clippy, focused, workspace, and diff gates are consistent with the reviewed commits.
- [x] A failing metacharacter-path probe left zero children in its dedicated temp parent, confirming `TempDir` cleanup did not traverse or leak the junction fixture; the empty probe parent was then removed.

## Resolved Findings

1. **Resolved — existing-pack verification preserves structured error contracts.** Repair `7fa41c1` replaces the text-only conversion with a contextual `XtaskError::ExistingPack { source: PackError }` and covers the typed source behavior.
2. **Resolved — required link/reparse coverage cannot silently pass.** Repair `7fa41c1` removes both early returns, requires a real native symlink or Windows junction, and asserts symlink/reparse metadata before exercising the verifier.

## Remaining Findings

1. **Medium — the Windows junction fallback passes filesystem paths through `cmd.exe` without command-shell escaping.** `crates/goldeneye-grammar-pack/tests/materialized_pack.rs:323`-`327` invokes `cmd /C mklink /J` and appends `Path` operands directly. `Command::arg` protects `CreateProcess` argument boundaries but does not neutralize `cmd.exe` metacharacters. With a valid dedicated `TEMP`/`TMP` path ending in `gfp1&probe`, both reparse tests failed because `cmd` parsed the ampersand as shell syntax; other metacharacter-bearing paths can likewise be split or expanded as commands. Avoid `cmd.exe` by creating the junction through a Windows API/library, and retain a focused temp-path regression containing shell metacharacters. If the shell must remain, use a dedicated, audited cmd-escaping routine rather than raw path arguments.
