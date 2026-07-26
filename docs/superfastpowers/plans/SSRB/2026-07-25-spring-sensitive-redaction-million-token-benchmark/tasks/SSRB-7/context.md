# Context for SSRB-7

**Plan:** `docs/superfastpowers/plans/SSRB/2026-07-25-spring-sensitive-redaction-million-token-benchmark.md`
**Task:** `SSRB-7`
**Commit SHA:** Final frozen benchmark candidate
`ba5876e693e580947481a166cba8910f7e81a9df`; documentation wrap-up is the
subsequent report commit.

## Starting Context

- `tools/agent-bench/qualification.mjs`: calibration and scored-lane token gates.
- `tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json`: initial Level-2 workload.
- `target/agent-bench/spring-sensitive-value-redaction-level2/calibration`: versioned non-scored attempts.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Added Level 1 and Level 0 workload variants after Level 2 and Level 1
  exceeded the 1.2M input ceiling.
- Final clean vanilla calibration: 993,518 input, 930,816 cached, 62,702
  uncached, grader PASS, zero protocol violations.
- User accepted the uncached-input gate exception and stopped further tuning.
- Best in-band uncached attempt: nested/indexed Level 0 at 87,415 uncached and
  1,158,007 input.
- Retained report:
  `docs/benchmarks/2026-07-26-spring-sensitive-value-redaction-benchmark.md`.
- Verification: 46 harness tests passed; candidate and pinned Spring
  repositories clean.
