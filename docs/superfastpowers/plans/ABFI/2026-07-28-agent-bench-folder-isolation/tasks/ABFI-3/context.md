# Context for ABFI-3

**Plan:** `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation.md`
**Task:** `ABFI-3`
**Commit SHA:** `6d51cf0` (`docs(bench): correct obsolete runner wording`).

## Starting Context

- `tools/agent-bench/isolation.test.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.
- `tools/agent-bench/package.json`: starting point for this task's isolation, migration, documentation, or verification scope.
- `tools/agent-bench/README.md`: starting point for this task's isolation, migration, documentation, or verification scope.

## Open Context Rule

Files above are starting points only. Inspect any additional file needed to complete task correctly.

## Completion Updates

- Task commit/range: `2eef8d4` (reviewed against predecessor `4b26507`).
- Files changed: `tools/agent-bench/isolation.test.mjs`, `tools/agent-bench/package.json`, and `tools/agent-bench/README.md`.
- Inspected dependency: `tools/agent-bench/core.test.mjs`; ABFI-2 repair `4b26507` made its paths package-CWD-independent before package-level verification.
- TDD evidence: package-contract test first failed with `ENOENT` for the missing manifest, then passed after the exact private ESM manifest was added.
- Verification passed: `node --test tools/agent-bench/isolation.test.mjs` (4/4), `npm --prefix tools/agent-bench test` (51/51), `npm --prefix tools/agent-bench run check`, and `git diff --check`.
- Notes: README documents the local commands, runtime ownership, inputs/outputs, and Cargo production boundary. The pre-existing untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md` was preserved.

## Quality Repair

- Task commits: `2eef8d4` and README-only repair `6d51cf0`; repair range `484853a..6d51cf0`.
- Corrected the production-boundary wording so it describes the removed top-level runner locations without falsely claiming the current `bin/` entrypoints are absent or reintroducing exact legacy path literals.
- Verification passed: both current `bin/` entrypoints exist; exact tracked legacy agent and competitor path matches are `0`; `npm --prefix tools/agent-bench test` passed 52/52; `npm --prefix tools/agent-bench run check` and `git diff --check` exited `0`.
- Preserved the call-tree artifact and existing plan-scoped review artifacts.
