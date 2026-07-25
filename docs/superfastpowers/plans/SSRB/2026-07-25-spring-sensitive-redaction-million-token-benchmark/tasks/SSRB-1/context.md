# Context for SSRB-1

**Plan:** `docs/superfastpowers/plans/SSRB/2026-07-25-spring-sensitive-redaction-million-token-benchmark.md`
**Task:** `SSRB-1`
**Commit SHA:** Pending final task commit; recorded in the implementation handoff. If review fixes add commits, update to the latest task commit and note the reviewed range below.

## Starting Context

- `tools/benchmark-agent-tasks.mjs`: runner finalizes dirty paths and protocol violations.
- `tools/agent-bench/core.mjs`: shared harness utilities and config loading.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Files created: `tools/agent-bench/path-policy.mjs`, `tools/agent-bench/path-policy.test.mjs`.
- Files modified: `tools/benchmark-agent-tasks.mjs`.
- Additional relevant files: this context file and the SSRB plan progression record.
- Verification: `node --test tools/agent-bench/path-policy.test.mjs` (PASS, 3 tests);
  `node --test tools/agent-bench/*.test.mjs` (PASS, 26 tests);
  `node --check tools/benchmark-agent-tasks.mjs` (PASS).
