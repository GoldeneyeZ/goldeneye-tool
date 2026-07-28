# Context for ABFI-4

**Plan:** `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation.md`
**Task:** `ABFI-4`
**Commit SHA:** `3cc0c46591ad21c0cf5655622b665928f6a835d3` (`docs(bench): migrate isolated entrypoint paths`).
**Reviewed range:** `2eef8d4..3cc0c46`

## Starting Context

- `tools/agent-bench/core.test.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.
- `FOLDER_STRUCTURE.md`: starting point for this task's isolation, migration, documentation, or verification scope.
- `docs/agent-task-benchmark.md`: starting point for this task's isolation, migration, documentation, or verification scope.
- `docs/benchmarks/2026-07-14-codebase-memory-vs-goldeneye.md`: starting point for this task's isolation, migration, documentation, or verification scope.

## Open Context Rule

Files above are starting points only. Inspect any additional file needed to complete task correctly.

## Completion Updates

- Migrated every tracked exact reference from both legacy benchmark entrypoints to `tools/agent-bench/bin/` paths, including moved-runner help text and historical plans.
- Verified `git grep` finds zero legacy-path references.
- Verification passed: `node --test tools/agent-bench/core.test.mjs` (16 tests), `node --test tools/agent-bench/isolation.test.mjs` (4 tests), `npm --prefix tools/agent-bench test` (51 tests), and `npm --prefix tools/agent-bench run check`.
- Reviewed the path-only diff (+115/-115 documentation-path replacement lines) and confirmed `git diff --check` passes.
