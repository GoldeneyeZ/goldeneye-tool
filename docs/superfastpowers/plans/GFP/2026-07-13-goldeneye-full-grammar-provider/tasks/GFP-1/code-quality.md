# GFP-1 Code-Quality Review

- Result: approved
- Reviewer: Codex code-quality review
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Baseline: plan `6e2b800`; design whitespace follow-up `023837d`
- Reviewed ranges: implementation `514f41a^..514f41a`; first repair re-review `27e28f7..64d8d6b` (code `7fa41c1`, record `64d8d6b`); final repair re-review `83a77d8..6adb82e` (code `5bddeea`, record `6adb82e`), after specification approval `26ab716`

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
- [x] Implementer evidence records passing formatting, focused Clippy, focused tests, workspace tests, and `git diff --check`. The final re-review reran `cargo test -p goldeneye-grammar-pack`: materialized-pack tests passed 13/13, with unit and doc tests also green.
- [x] Original structured-error finding resolved: `XtaskError::ExistingPack` retains `PackError` as its typed `#[source]` and adds existing-destination context. The regression test asserts the outer variant, `PackError::HashMismatch`, display context, and downcast source chain. A focused re-run passed 1/1.
- [x] Original silent-skip finding resolved: both link/reparse tests now require fixture creation and assert the final path has symlink/reparse metadata. Captured mutation runs show forced fixture failure made each test fail, so the RED evidence is truthful.
- [x] On the normal Windows temp path, native error 1314 exercised the junction fallback and both focused reparse tests passed 2/2 with their metadata assertions. The recorded post-repair formatting, Clippy, focused, workspace, and diff gates are consistent with the reviewed commits.
- [x] A failing metacharacter-path probe left zero children in its dedicated temp parent, confirming `TempDir` cleanup did not traverse or leak the junction fixture; the empty probe parent was then removed.
- [x] The shell-path finding is resolved: the junction helper calls `junction::create` directly, and no `cmd`, `mklink`, or process invocation remains in the fixture path. The `junction&probe` regression treats `&` as ordinary path data, asserts reparse metadata, removes only the junction, and proves its target remains intact.
- [x] Failed junction creation is cleaned without deleting a pre-existing path: cleanup is enabled only when `symlink_metadata` proved the link absent before the call. The pinned crate's overlong-target failure creates a partial directory before returning `InvalidInput`; the regression proves the helper removes that artifact and leaves the temporary directory empty. Existing paths make the absence predicate false and cannot enter the cleanup branch.
- [x] Native Windows behavior remains symlink-first for both file and directory fixtures. Only `PermissionDenied`/OS error 1314 enters the junction fallback, and the verifier tests still assert symlink/reparse metadata before rejection.
- [x] Dependency scope is narrow and reproducible: `junction = "=2.0.0"` is a Windows-only dev-dependency with default features disabled. The resolved feature tree contains `junction` and `scopeguard` but not `unstable_admin`; the normal/build dependency tree contains no `junction`. Local package metadata reports the dependency as MIT licensed and Windows dev-only.
- [x] Repair `5bddeea` changes only the pack test manifest/lock and materialized-pack tests; record `6adb82e` changes only GFP-1 context/handoff. Their RED/GREEN and final-gate claims agree with captured test evidence and the reviewed tree.

## Resolved Findings

1. **Resolved — existing-pack verification preserves structured error contracts.** Repair `7fa41c1` replaces the text-only conversion with a contextual `XtaskError::ExistingPack { source: PackError }` and covers the typed source behavior.
2. **Resolved — required link/reparse coverage cannot silently pass.** Repair `7fa41c1` removes both early returns, requires a real native symlink or Windows junction, and asserts symlink/reparse metadata before exercising the verifier.
3. **Resolved — Windows junction paths no longer pass through a command shell.** Repair `5bddeea` replaces `cmd /C mklink /J` with the pinned direct Windows junction API, covers shell metacharacters as path data, and cleans known partial artifacts without removing pre-existing paths.

## Remaining Findings

No remaining code-quality or dependency findings. GFP-1 is approved.
