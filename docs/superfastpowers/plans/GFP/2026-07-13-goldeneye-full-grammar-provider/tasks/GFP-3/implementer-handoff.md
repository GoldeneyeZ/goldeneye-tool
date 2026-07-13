# GFP-3 Implementer Handoff

Status: Pending.

References: plan `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`, design `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`, plan commit `6e2b800`, design whitespace follow-up `023837d`.

Implementation scope:

- Execute the exact self-contained task in `task.md` using TDD.
- Implement the opt-in native build, deterministic wrappers, verified-cache boundary, confined FFI, and safe static registry.
- Touch only the files named by GFP-3 unless a genuine blocker requires plan-level escalation.

Constraints to preserve:

- Default builds remain cache-independent.
- Exactly 159 wrappers are planned, with 157 exposed factories and both ObjectScript records unavailable.
- All locked factory/scanner symbols are prefixed; no whole-archive flags or runtime hash map are introduced.
- No source fetch, copy, flatten, patch, or unverified compilation occurs in `build.rs`.
- GFP-2 must be complete and approved before implementation begins.

Completion gates:

- Record RED evidence before implementation.
- Record focused native, negative-path, determinism, helper-layout, and default-lane command outcomes.
- Record the resulting commit only after all gates pass.

Handoff evidence: Pending. No implementation work or command results are claimed.
