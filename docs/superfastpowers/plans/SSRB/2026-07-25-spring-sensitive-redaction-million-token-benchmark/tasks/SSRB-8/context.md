# Context for SSRB-8

**Plan:** `docs/superfastpowers/plans/SSRB/2026-07-25-spring-sensitive-redaction-million-token-benchmark.md`
**Task:** `SSRB-8`
**Commit SHA:** Final frozen benchmark candidate
`ba5876e693e580947481a166cba8910f7e81a9df`; documentation wrap-up is the
subsequent report commit.

## Starting Context

- `tools/agent-bench/bin/benchmark-agent-tasks.mjs`: six-run execution and audit CLI.
- `target/agent-bench/spring-sensitive-value-redaction-level2/qualification-selection.json`: chosen frozen workload.
- `target/agent-bench/spring-sensitive-value-redaction-level2/provenance.json`: seed and audit inputs.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Clean GCAL smoke and clean vanilla calibration both passed the same held-out
  grader on the final frozen task and snapshot.
- Final pair: GCAL 984,009 input / 7.16 min; vanilla 993,518 input / 5.29 min.
- User explicitly waived the randomized 3×3 matrix after accepting the final
  clean `n = 1` result.
- Sample SD and CV remain undefined; no statistical-significance claim.
- Raw `target/agent-bench/**` artifacts remain local and intentionally
  uncommitted.
- Published report:
  `docs/benchmarks/2026-07-26-spring-sensitive-value-redaction-benchmark.md`.
