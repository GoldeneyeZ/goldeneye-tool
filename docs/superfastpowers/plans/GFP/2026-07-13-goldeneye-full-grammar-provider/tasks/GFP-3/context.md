# GFP-3 Context

Status: Pending.

Authoritative inputs:

- Plan: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`
- Design: `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Plan commit: `6e2b800`
- Design whitespace follow-up: `023837d`

Scope:

- Add an opt-in `compiled` feature and deterministic native build plan to `goldeneye-full-grammars`.
- Verify the materialized grammar pack before generating or compiling 159 wrappers.
- Namespace the locked grammar factories and standard external-scanner exports under `goldeneye_full_*`.
- Confine unsafe FFI to the generated module and expose safe, static registry queries.
- Update the workspace manifests and lockfile required by the native crate.

Constraints:

- The default feature lane must not require or inspect `GOLDENEYE_GRAMMAR_PACK_DIR`.
- Build-time source acquisition, mutation, flattening, and patching are forbidden.
- Unsupported scanners and stale, extra, or hash-drifted assets must fail before compiler invocation.
- Helpers are included by wrappers and are not independent compilation units.
- Lookup uses binary search; public APIs do not expose raw functions or pointers.
- The task begins only after GFP-2 has passed both review gates.

Required gates:

- Focused RED test for the missing compiled registry/build behavior.
- Fresh-cache native compilation and link verification for 159 wrappers and 157 factories.
- Missing-cache, stale-cache, helper-layout, determinism, and ObjectScript-absence checks.
- Default-lane `cargo check`, workspace Clippy/tests, and `git diff --check` after clearing full-pack state.

Evidence: Pending. No implementation, test, review, or commit evidence has been recorded.
