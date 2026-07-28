# Context for SUWB-4

**Plan:** `docs/superfastpowers/plans/SUWB/2026-07-24-spring-unicode-warm-benchmark.md`
**Task:** `SUWB-4`
**Commit SHA:** `8dec5b1` (final smoke-eligible benchmark candidate).

## Starting Context

- `tools/agent-bench/bin/benchmark-agent-tasks.mjs`: preparation, smoke, and verification entrypoints.
- `tools/agent-bench/snapshot.mjs`: immutable snapshot implementation.
- `tools/agent-bench/configs/spring-stringutils-unicode-truncate.json`: frozen benchmark configuration.
- `D:\Dev\IdeaProjects\agent-context-kernel`: frozen ACK candidate repository.
- `target\release\goldeneye.exe`: frozen Goldeneye candidate binary.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Final task commit SHA: `8dec5b1`
- Reviewed commit range: `4566b07..8dec5b1`
- Files created: `tools/agent-bench/provenance.mjs`, `tools/agent-bench/provenance.test.mjs`
- Files modified: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`, `tools/agent-bench/core.mjs`, `tools/agent-bench/core.test.mjs`, `tools/agent-bench/snapshot.mjs`, Goldeneye runtime/index/bootstrap/CLI crates, benchmark config and prompt
- Additional relevant files: `target/agent-bench/spring-stringutils-unicode-truncate/preparation.json`, `target/agent-bench/snapshots/spring-stringutils/snapshot-manifest.json`
- Verification commands/results: full-grammar clippy/test/release gates all exited 0; harness suite passed 35/35; final snapshot contained exactly 2 indexed source files, 388 nodes, and 513 edges; smoke was eligible with Codex exit 0, grader exit 0, two allowed dirty paths, zero protocol violations, and unchanged candidate/source/snapshot fingerprints.
- Notes: Path-exact file-node identity removed the duplicate-node blocker. Runtime SQLite quiescence, ACK shutdown tolerance, bounded full-Java grammar selection, exact two-file discovery filtering, worktree readiness/cleanup retries, and evidence-preserving failures made preparation repeatable. Snapshot manifest SHA-256 used for scoring: `b60ce99c815bca2930e6d12d7fca2ce466ebadb219219f34a4a8adcece10ea22`.
