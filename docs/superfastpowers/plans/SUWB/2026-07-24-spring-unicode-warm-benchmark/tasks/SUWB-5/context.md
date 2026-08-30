# Context for SUWB-5

**Plan:** `docs/superfastpowers/plans/SUWB/2026-07-24-spring-unicode-warm-benchmark.md`
**Task:** `SUWB-5`
**Commit SHA:** `8dec5b1` (scored candidate and lane-specific prompt freeze).

## Starting Context

- `tools/agent-bench/bin/benchmark-agent-tasks.mjs`: scoring, reporting, and audit entrypoint.
- `target/agent-bench/spring-stringutils-unicode-truncate/preparation.json`: required scoring-eligibility evidence.
- `target/agent-bench/snapshots/spring-stringutils/snapshot-manifest.json`: immutable snapshot evidence.
- `tools/agent-bench/configs/spring-stringutils-unicode-truncate.json`: frozen run configuration.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Final task commit SHA: `8dec5b1`
- Reviewed commit range: `34170ef..8dec5b1`
- Files created: `tools/agent-bench/report.mjs`, `tools/agent-bench/report.test.mjs`, four scored run artifact sets, `target/agent-bench/spring-stringutils-unicode-truncate/report.json`, `target/agent-bench/spring-stringutils-unicode-truncate/report.md`
- Files modified: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`, `tools/agent-bench/core.mjs`, `tools/agent-bench/core.test.mjs`, benchmark config and task prompt
- Additional relevant files: `target/agent-bench/spring-stringutils-unicode-truncate/invalid-attempts/vanilla-global-gcal-20260725T0318/`
- Verification commands/results: pre-run verification passed; vanilla 1/1 passed; Goldeneye+GCAL 3/3 passed serially; every run had grader exit 0, two allowed dirty paths, and zero protocol violations; post-run verification passed; `--audit-report` passed with four traceable runs, one vanilla, three candidates, identical candidate snapshot hash, timing identities, unchanged fingerprints, complete raw artifacts, and required limitations text.
- Notes: Candidate wall times were 45,743 ms, 54,922 ms, and 74,580 ms (median 54,922 ms). Vanilla wall time was 48,963 ms and is descriptive reuse evidence only. One invalid vanilla attempt that called global GCAL was stopped before reporting and preserved under `invalid-attempts`; the task-level GCAL instruction was removed so lane-specific protocol instructions control discovery.
