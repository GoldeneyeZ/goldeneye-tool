# Context for SUWB-2

**Plan:** `docs/superfastpowers/plans/SUWB/2026-07-24-spring-unicode-warm-benchmark.md`
**Task:** `SUWB-2`
**Commit SHA:** `8e6fa7e`.

## Starting Context

- `tools/benchmark-agent-tasks.mjs`: existing scored-run orchestration and Codex process launch.
- `tools/agent-bench/core.mjs`: existing config, run-matrix, and duration logic.
- `tools/agent-bench/core.test.mjs`: existing harness tests.
- `tools/agent-bench/snapshot.mjs`: SUWB-1 snapshot lifecycle.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Final task commit SHA: `8e6fa7e` (`fix(bench): stop wall timing at Codex exit`)
- Reviewed commit range: `4d3fd13..8e6fa7e`
- Files created: `tools/agent-bench/timing.mjs`, `tools/agent-bench/timing.test.mjs`
- Files modified: `tools/agent-bench/core.mjs`, `tools/agent-bench/core.test.mjs`, `tools/benchmark-agent-tasks.mjs`
- Additional relevant files: `tools/agent-bench/snapshot.mjs` (SUWB-1 containment, copy, manifest, and restore APIs consumed without modification)
- Verification commands/results: initial watched RED: `node --test tools/agent-bench/timing.test.mjs` failed with `ERR_MODULE_NOT_FOUND`; GREEN passed (2 tests). Initial core RED: `node --test tools/agent-bench/core.test.mjs` failed because `resolveRunLayout` was not exported; GREEN passed (11 tests). Repair RED: timing test failed because `stopTimerAtClose` was not exported; core test failed because `resolveRepositoryGate` was not exported. Repair GREEN: timing passed (3 tests), core passed (11 tests), and `node --test tools/agent-bench/*.test.mjs` passed (21 tests). `git diff --cached --check` passed before both commits.
- Notes: Repair `8e6fa7e` captures `wall_ms` inside terminal child `close`/`error` callbacks, before stream flushing, and hard-gates ready-snapshot scoring on the recreated stable worktree rather than source repository. Documentation/progression metadata intentionally remains uncommitted.
