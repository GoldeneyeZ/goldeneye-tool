# GFP-1 Implementer Handoff

- Status: implementation and both requested code-quality repairs complete; specification approved; code-quality re-review pending
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Baseline: plan `6e2b800`; design whitespace follow-up `023837d`
- Implementation commit: `514f41a`
- Specification review commit: `26ab716` (`approved`)
- Code-quality review commit: `27e28f7` (`changes requested`)
- Repair commit: `7fa41c1`
- Code-quality re-review commit: `83a77d8` (`changes requested`)
- Junction repair commit: `5bddeea`

## Scope

Create the safe pack-integrity crate, centralize read-only pack-state/layout verification there, rewire syntax and xtask without type duplication, and preserve atomic writes in xtask.

## Constraints

No behavior drift, unsafe/native/runtime dependency expansion, cache requirement in the default lane, or fabricated compatibility type is allowed.

## Required Gates

Run the GFP-1 focused RED/GREEN tests, focused Clippy, workspace tests, formatting, and `git diff --check` exactly as listed in `task.md`.

## Handoff

Re-review implementation commit `514f41a` plus repair commits `7fa41c1` and `5bddeea`. The pack lock and exact-Git implementation moved into `goldeneye-grammar-pack`; materialized state/layout/hash verification is now shared there; syntax publicly re-exports the exact pack-crate types; and xtask retains state writing and atomic publication while depending on the pack crate directly. The first repair preserves `PackError` as the typed source of `XtaskError::ExistingPack` and makes both link/reparse tests require a real fixture. The second repair removes the `cmd.exe` junction boundary, pins the safe Windows API dependency without default features, and cleans partial link paths on failed creation.

The code-quality review remains `changes requested` until a reviewer updates it. The specification-review and code-quality files were untouched by the repair implementer.

## Evidence

- RED: syntax grammar-lock tests failed with unresolved `goldeneye_grammar_pack`; pack materialized tests failed while the crate manifest was absent.
- GREEN: pack materialized tests `13/13`, syntax grammar-lock tests `8/8`, and xtask grammar-sync tests `16/16` passed after both repair regressions were added.
- A focused Clippy failure for `used_underscore_binding` was reproduced, traced to an accessed `_temporary` fixture field, fixed by the minimal `temporary` rename, and rerun successfully.
- Repair RED/GREEN: the existing-pack corruption regression first failed to compile with `E0599` because `XtaskError::ExistingPack` was absent. It passed after the variant retained `PackError::HashMismatch` as its `#[source]` and kept `existing destination` context in the outer error.
- Repair RED/GREEN: forcing link creation failure made both focused tests fail, proving they can no longer silently skip. Native Windows symlink creation reproduced OS error 1314; the fixture now uses an unprivileged junction fallback, verifies the reparse-point attribute, and passes both focused tests with real reparse fixtures.
- Junction repair RED/GREEN: `junction&probe` failed under the old helper because `cmd.exe` split the path at `&` and attempted to execute `probe`; it passed after direct `junction::create` replaced `cmd /C mklink /J`.
- Junction cleanup RED/GREEN: an overlong target left `partial-junction` behind when creation returned `InvalidInput`; the helper now performs guarded best-effort cleanup, and the regression proves the link path and temporary directory are empty afterward.
- `junction` is a Windows-target-specific dev dependency pinned to `2.0.0` with default features disabled; the resolved feature tree contains no `unstable_admin` feature.
- Fresh final gates passed: formatting, focused Clippy, all pack tests, syntax grammar-lock tests, xtask grammar-sync tests, full workspace tests, and `git diff --check`.
- Staged-scope and dependency audits found only GFP-1 implementation paths, no duplicate xtask pack-state/layout verifier, and no forbidden pack-crate dependency.
