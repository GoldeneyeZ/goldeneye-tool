# Context for ABFI-2

**Plan:** `docs/superfastpowers/plans/ABFI/2026-07-28-agent-bench-folder-isolation.md`
**Task:** `ABFI-2`
**Commit SHA:** `484853ad2e20ea7adc2287f5e9e19c0fd177419d` (`fix(bench): preserve competitor workspace root`).
**Reviewed range:** `e4b8bb5..484853a`

## Starting Context

- `tools/agent-bench/bin/benchmark-agent-tasks.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.
- `tools/agent-bench/bin/benchmark-competitors.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.
- `tools/agent-bench/core.test.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.
- `tools/agent-bench/isolation.test.mjs`: starting point for this task's isolation, migration, documentation, or verification scope.

## Open Context Rule

Files above are starting points only. Inspect any additional file needed to complete task correctly.

## Completion Updates

- Moved the agent-task and competitor runners into `tools/agent-bench/bin/`; the agent runner now uses only in-boundary local imports.
- Updated all three direct core-test invocations of the agent runner to its relocated path.
- Preserved runner behavior with the required location-relative repository-root calculation and provenance self-reference updates; both were necessary after moving the file two directories deeper.
- Inspected the moved runner, competitor runner, `core.mjs`, `provenance.mjs`, `core.test.mjs`, and the ABFI-1 isolation contract.
- Verification: observed RED with `node --test tools/agent-bench/isolation.test.mjs` before the move (`1` pass, `2` failures for missing bin entrypoints); after the move, both `node --check` commands exited `0`, isolation passed (`3`/`3`), and `node --test tools/agent-bench/core.test.mjs` passed (`16`/`16`).
- The runner help text intentionally retains its old literal path until ABFI-4 performs the plan's tracked-reference migration. The untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md` was not added.
- Quality-only repair: package-local test execution exposed four `process.cwd()`-dependent paths in `core.test.mjs`; anchored the runner, fixture/config, and spawned runner CWD to `import.meta.url` instead.
- Repair verification: reproduced `npm --prefix tools/agent-bench test` failing `4` tests with doubled `tools/agent-bench` paths, then passed root core tests (`16`/`16`), package-local tests (`51`/`51`, including uncommitted ABFI-3 package coverage), isolation (`4`/`4`), and both runner syntax checks.
- Plan-quality repair: added `competitors.test.mjs` coverage deriving the competitor runner's actual workspace expression and proving its Goldeneye/output defaults and Cargo build CWD remain rooted at the repository after relocation.
- Competitor RED→GREEN evidence: the focused test first failed because `..` resolved to `tools/agent-bench`, then passed after changing the runner workspace derivation to `../../..`.
- Repair verification: focused competitor test passed (`1`/`1`), package tests passed (`52`/`52`), isolation passed (`4`/`4`), core tests passed (`16`/`16`), `npm --prefix tools/agent-bench run check` passed, and `git diff --check` passed.
