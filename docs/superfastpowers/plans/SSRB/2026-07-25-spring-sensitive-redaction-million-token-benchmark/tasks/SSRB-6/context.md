# Context for SSRB-6

**Plan:** `docs/superfastpowers/plans/SSRB/2026-07-25-spring-sensitive-redaction-million-token-benchmark.md`
**Task:** `SSRB-6`
**Commit SHA:** `b32ec4d46232c44162ee8ca2dd3ee536f1697d21`

## Starting Context

- `tools/agent-bench/provenance.mjs`: candidate/task/grader fingerprinting.
- `tools/agent-bench/snapshot.mjs`: immutable snapshot manifest creation and verification.
- `target/agent-bench/spring-sensitive-value-redaction-level2/preparation.json`: generated gate evidence.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

The implementer updates this section before review with the final task commit SHA, reviewed commit range if relevant, files created, files modified, additional relevant files, and verification commands/results.

### 2026-07-25 implementation evidence

- Candidate source commit: `b32ec4d46232c44162ee8ca2dd3ee536f1697d21` (reviewed harness-fix range: `ab1bd2b..b32ec4d`).
- Generated, intentionally uncommitted artifacts: `target/agent-bench/spring-sensitive-value-redaction-level2/preparation.json` and `provenance.json`.
- The full Node harness, held-out Spring grader, whitespace check, current `goldeneye` package tests, release build, snapshot creation/restore, and `--verify-only` passed. The task's literal `cargo test -p goldeneye-code-agent-layer` command is obsolete because that Cargo package does not exist; `cargo test -p goldeneye` passed instead.
- The Spring configuration sets `GOLDENEYE_GRAMMAR_PACK=full`, so the frozen candidate was built with `--features full-grammar-pack` and the local grammar-pack directory. The basic release build is insufficient for that configured runtime.
- Audit found that configured include directories were previously treated as exact file names, producing an invalid 393,341-byte snapshot with only 2 nodes and 1 edge. Commit `b32ec4d` adds component-aware tree-prefix matching with a regression test.
- The rebuilt featured candidate is 244,390,400 bytes with SHA-256 `62fa214bf146ef99dfb3ae9236299934ee0c25190fc921ad1928ff8581d885de`.
- The corrected snapshot manifest SHA-256 is `fbf2c2ddabf84df33abfc12dcc530d8413dd3c1ed8d42a0a0025999652c8e942`: 459,083,901 bytes across `goldeneye.db` and `gcal-state/projects.json`, containing 4,693 files, 212,047 nodes, and 256,956 edges. Restore and `--verify-only` passed against Spring `daf955157871e4ac6f192e06b71d6cc595eb979b`.
- Frozen candidate and snapshot verification passed. Scoring eligibility is deliberately `false`: it additionally requires a clean `--smoke` result, and the external Codex quota was exhausted before the agent turn. No smoke result was fabricated.
