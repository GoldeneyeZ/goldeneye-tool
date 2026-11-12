# Context for SSRB-5

**Plan:** `docs/superfastpowers/plans/SSRB/2026-07-25-spring-sensitive-redaction-million-token-benchmark.md`
**Task:** `SSRB-5`
**Commit SHA:** Pending until task completion. If review fixes add commits, update to the latest task commit and note the reviewed range below.

## Starting Context

- `tools/agent-bench/configs/spring-stringutils-unicode-truncate.json`: prior pinned Spring snapshot config.
- `tools/agent-bench/bin/benchmark-agent-tasks.mjs`: dry-run matrix and snapshot preparation entry points.

## Open Context Rule

The files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

The implementer updates this section before review with the final task commit SHA, reviewed commit range if relevant, files created, files modified, additional relevant files, and verification commands/results.

- Task implementation commit: `4f308dcc67c567a98bc486e8642d80af879a944d` (`bench: configure million token Spring task`).
- Files created: `tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json`.
- Files modified: `tools/agent-bench/core.test.mjs`.
- Generated, intentionally uncommitted: `target/agent-bench/snapshots/spring-sensitive-value-redaction-level2/**` and `target/agent-bench/spring-sensitive-value-redaction-level2/{preparation,provenance}.json`.
- RED: `node --test tools/agent-bench/core.test.mjs` failed only because the Level-2 config did not yet exist (`ENOENT`).
- GREEN: the same command passed 16/16 after the pinned config was added; `node --test tools/agent-bench/*.test.mjs` passed 45/45; `pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1` passed 4/4.
- Dry-run: the fixed seed `20260725` produced six unique runs: three `goldeneye-code-agent-layer/warm` and three `vanilla/none`.
- Snapshot: source target was clean at `daf955157871e4ac6f192e06b71d6cc595eb979b`; final restore verification passed with manifest SHA-256 `47c4083fff34376a91523d91f234fba8195ac0a7de0458f94167cba761e6eca8`, two files, and 393,341 bytes. The initial long worktree path hit Windows `MAX_PATH`; the pinned config now uses the unique short paths `.gab\\ssrb5-wt` and `.gab-cache\\ssrb5-live`.
- Plan variance: the Task-5 command uses only `--prepare-snapshot`, while the current runner computes `eligible_for_scoring` only after its separate `--smoke` full Codex-and-grader run. Consequently the snapshot-only preparation and `--verify-only` correctly report `NOT ELIGIBLE` despite passing restore and source gates. Root will run the mandatory smoke gate separately; no runner semantics were changed by SSRB-5.
