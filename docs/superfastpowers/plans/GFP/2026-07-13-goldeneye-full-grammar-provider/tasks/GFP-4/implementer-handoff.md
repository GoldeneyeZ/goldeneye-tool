# GFP-4 Implementer Handoff

Status: Pending.

References: plan `docs/superfastpowers/plans/GFP/2026-07-13-goldeneye-full-grammar-provider.md`, design `docs/superfastpowers/specs/2026-07-13-goldeneye-full-grammar-provider-design.md`, plan commit `6e2b800`, design whitespace follow-up `023837d`.

Implementation scope:

- Execute the exact self-contained task in `task.md` using TDD.
- Add explicit core/full features, the safe provider, ABI mismatch reporting, and the required runtime/feature-graph tests.
- Touch only the files named by GFP-4 unless a genuine blocker requires plan-level escalation.

Constraints to preserve:

- Core grammars remain enabled by default; full grammars remain opt-in.
- The full-only graph excludes maintained core grammar crates, while all-features must link both suites safely.
- Runtime queries expose 159 supported IDs and 157 unique grammars while excluding ObjectScript.
- Full lookups preserve locked metadata and fail safely on unsupported IDs or ABI drift.
- GFP-3 must be complete and approved before implementation begins.

Completion gates:

- Record RED evidence before implementation.
- Record full-only tests/Clippy, mixed-link results, feature-tree evidence, and the cache-free default regression lane.
- Record the resulting commit only after all gates pass.

Handoff evidence: Pending. No implementation work or command results are claimed.
