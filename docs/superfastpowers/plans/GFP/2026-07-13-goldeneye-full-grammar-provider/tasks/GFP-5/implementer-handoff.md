# GFP-5 Implementer Handoff

Status: Pending.

References: plan `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`, design `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`, plan commit `6e2b800`, design whitespace follow-up `023837d`.

Implementation scope:

- Execute the exact self-contained task in `task.md` using TDD.
- Add the isolated full-pack CI lane, tracked claim guards, operator documentation, and third-party boundary clarification.
- Touch only the files named by GFP-5 unless a genuine blocker requires plan-level escalation.

Constraints to preserve:

- The existing default platform matrix and core-only gates remain intact.
- Full-pack CI pins the upstream SHA, acquires inputs before offline mode, verifies materialized assets, and uses explicit full features/cache state.
- Documentation states the `160/159/157/2` model, symbol namespacing, full-only artifact features, no build-time downloads, and Phase 6 packaging boundary.
- No cache shortcut, broad restore key, core-only evidence inflation, or unsupported release ceiling is introduced.
- GFP-4 must be complete and approved before implementation begins.

Completion gates:

- Record RED evidence before implementation.
- Record CI contract, regeneration, default/full/mixed/release command outcomes, and observational baselines.
- Record the resulting commit only after all gates pass.

Handoff evidence: Pending. No implementation work, command results, baselines, or commit are claimed.
