# Context for ABFI-1

**Plan:** `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation.md`
**Task:** `ABFI-1`
**Commit SHA:** `e4b8bb5eb1b5d2436e27bed58dfead89099cb54f` (`test(bench): require one-folder runtime isolation`).
**Reviewed range:** `b4acc41..e4b8bb5`

## Starting Context

- `tools/agent-bench/isolation.test.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.
- `tools/agent-bench/bin/benchmark-agent-tasks.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.
- `tools/agent-bench/bin/benchmark-competitors.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.

## Open Context Rule

Files above are starting points only. Inspect any additional file needed to complete task correctly.

## Completion Updates

- Created `tools/agent-bench/isolation.test.mjs` as the permanent physical-isolation and Rust production-boundary contract.
- Inspected the legacy entrypoints `tools/agent-bench/bin/benchmark-agent-tasks.mjs` and `tools/agent-bench/bin/benchmark-competitors.mjs`; both exist, while `tools/agent-bench/bin/` does not yet exist.
- Verification: `node --test tools/agent-bench/isolation.test.mjs` exited `1` as intended: the two entrypoint tests failed because the new `bin/` entrypoints are missing, and the production Rust-boundary test passed (`1` pass, `2` failures).
- No production code was changed. The contract is intentionally RED until ABFI-2 relocates both entrypoints.
