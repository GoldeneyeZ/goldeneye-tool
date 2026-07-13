# GFP-1 Implementer Handoff

- Status: pending
- Plan/design: `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md` and `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`
- Baseline: plan `6e2b800`; design whitespace follow-up `023837d`

## Scope

Create the safe pack-integrity crate, centralize read-only pack-state/layout verification there, rewire syntax and xtask without type duplication, and preserve atomic writes in xtask.

## Constraints

No behavior drift, unsafe/native/runtime dependency expansion, cache requirement in the default lane, or fabricated compatibility type is allowed.

## Required Gates

Run the GFP-1 focused RED/GREEN tests, focused Clippy, workspace tests, formatting, and `git diff --check` exactly as listed in `task.md`.

## Handoff

No repair brief exists. Begin GFP-1 from Step 1 using TDD.

## Evidence

Pending; implementation has not started.
