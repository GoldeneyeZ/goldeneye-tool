# Context for SSRB-6

**Plan:** `docs/superfastpowers/plans/SSRB/2026-07-25-spring-sensitive-redaction-million-token-benchmark.md`
**Task:** `SSRB-6`
**Commit SHA:** Pending until task completion. If review fixes add commits, update to the latest task commit and note the reviewed range below.

## Starting Context

- `tools/agent-bench/provenance.mjs`: candidate/task/grader fingerprinting.
- `tools/agent-bench/snapshot.mjs`: immutable snapshot manifest creation and verification.
- `target/agent-bench/spring-sensitive-value-redaction-level2/preparation.json`: generated gate evidence.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

The implementer updates this section before review with the final task commit SHA, reviewed commit range if relevant, files created, files modified, additional relevant files, and verification commands/results.

### 2026-07-25 implementation evidence

- Candidate source commit: `81507215e426dac1baec7b30124b8b4ebc4be283` (reviewed harness-fix range: `ab1bd2b..8150721`).
- Generated, intentionally uncommitted artifacts: `target/agent-bench/spring-sensitive-value-redaction-level2/preparation.json` and `provenance.json`.
- The full Node harness, held-out Spring grader, whitespace check, current `goldeneye` package tests, release build, snapshot creation/restore, and `--verify-only` passed. The task's literal `cargo test -p goldeneye-ack` command is obsolete because that Cargo package does not exist; `cargo test -p goldeneye` passed instead.
- The Spring configuration sets `GOLDENEYE_GRAMMAR_PACK=full`, so the frozen candidate was built with `--features full-grammar-pack` and the local grammar-pack directory. The basic release build is insufficient for that configured runtime.
- Frozen candidate and snapshot verification passed. Scoring eligibility is deliberately `false`: it additionally requires a clean `--smoke` result, and the external Codex quota was exhausted before the agent turn. No smoke result was fabricated.
