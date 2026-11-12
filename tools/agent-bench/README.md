# Goldeneye agent benchmark tools

All benchmark runtime code, configs, tasks, graders, and fixtures live in this directory. Production Cargo packages do not import this directory.

## Requirements

- Node.js 20 or newer
- Benchmark-specific external repositories, commands, and environment variables required by the selected config

## Commands

From repository root:

```powershell
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
npm --prefix tools/agent-bench run benchmark:agents -- <arguments>
npm --prefix tools/agent-bench run benchmark:competitors -- <arguments>
npm --prefix tools/agent-bench run benchmark:ops -- <arguments>
```

Direct entrypoints:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs <arguments>
node tools/agent-bench/bin/benchmark-competitors.mjs <arguments>
node tools/agent-bench/bin/benchmark-ops.mjs <arguments>
```

### Fast one-shot run

Use `--one-shot` for one task, one engine, one cache mode, one Codex invocation, and one held-out grader invocation:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level0.json `
  --one-shot `
  --task spring-sensitive-value-redaction-level0 `
  --engine goldeneye-code-agent-layer `
  --cache-modes warm `
  --repetitions 1 `
  --model gpt-5.6-luna
```

One-shot attempts are standalone and unqualified. They skip agent-side build, compile, test, lint, and check commands; GCAL discovery calls remain unlimited. A missing or stale warm GCAL snapshot is refreshed locally without a smoke/model run. By default, output is isolated at `target/agent-bench/<task-id>/one-shot/<attempt-id>/report.json`; use `--attempt-id <id>` for a stable name or `--out <path>` for an explicit report. Canonical scored reports are never read or merged.

### Operation-level run (no model)

`benchmark:ops` compares GCAL with plain shell discovery one information need at a time, without invoking a model. Each scenario in `ops/<corpus>.json` names a need, the evidence that proves it was met, and one scripted command chain per lane; a lane stops at the first step whose cumulative output holds all evidence. It runs in seconds and costs nothing, so use it to find and verify surface-level regressions before spending model tokens on an agent run.

```powershell
node tools/agent-bench/bin/benchmark-ops.mjs `
  --scenarios tools/agent-bench/ops/spring.json `
  --repo D:\path\to\spring-framework `
  --state target/ops-bench/spring `
  --gcal-entry C:\path\to\goldeneye-code-agent-layer\dist\main.js `
  --goldeneye target/release/goldeneye.exe `
  --rg-dir C:\path\to\dir-containing-rg `
  --label optimized
```

`--state` must hold an `gcal-home/` in which the repository was registered with `gcal init` (run with `GCAL_HOME=<state>/gcal-home` and `GOLDENEYE_DB_PATH=<state>/goldeneye.db`). Reports are written to `<state>/report-<label>.json` and `.md`. Run it once per GCAL build or Goldeneye binary and compare labels.

Reported per lane: calls, failed calls, output tokens (characters / 4), wall time, and a cost in uncached-token equivalents: `calls x 25000 x 0.1 + output tokens x 2`. The first term is the cached context a model re-reads on the extra turn each call causes; the second is the output read once uncached and re-read cached on about ten later turns. Under this model one extra call costs as much as roughly 5 KB of output, which is why call count, not output size, decides most scenarios.

A step written as `{ "cmd": ..., "when": "after-failure" }` is a recovery attempt and runs only once an earlier step of the chain exited non-zero. GCAL chains start from the name an agent would naturally type; vanilla chains are deliberately tight (`rg` with bounded context and exact globs), so the comparison is conservative for GCAL.

### Task variants

A task may start from a dirty working tree and may be graded on what the agent writes instead of on tests:

```json
{
  "id": "explain-binding-errors-dirty",
  "prompt_file": "../tasks/explain-binding-errors.md",
  "overlay": { "patch_file": "../overlays/explain-binding-errors.patch" },
  "grader": {
    "command": "node",
    "args": [
      "../graders/fact-checklist.mjs",
      "--checklist", "../graders/fixtures/explain-binding-errors.checklist.json",
      "--run-dir", "{runDir}",
      "--worktree", "{worktree}"
    ]
  }
}
```

- `overlay.patch_file` is applied with `git apply` after the index is primed or restored and before the agent starts, and is left uncommitted. A code-memory engine therefore works on a tree that changed after indexing, and every lane sees the same unrelated changes in `git status` and `git diff`. Paths the overlay leaves dirty are exempt from the dirty-path policy and counted in `overlay_paths`; `patch.diff` and the patch statistics include the overlay.
- `graders/fact-checklist.mjs` checks an answer against required and forbidden facts (case-insensitive regular expressions). `"source": "final-message"` reads the `summary` of the agent's final message, for exploration questions; any other value is a path inside the worktree, for documentation tasks. The checklist format is described at the top of the script.

## Layout

- `bin/`: benchmark entrypoints
- `configs/`: benchmark configurations
- `graders/`: grading code and injected fixtures
- `ops/`: operation-level scenario suites, one file per corpus
- `tasks/`: agent task prompts
- root modules: shared harness, timing, provenance, qualification, reporting, snapshot, and operation-level (`ops.mjs`) logic

## Inputs and outputs

Entrypoint arguments and environment variables remain defined by each runner and config. Generated benchmark evidence belongs under configured output directories, normally `target/agent-bench/`; generated evidence is not production runtime input.

## Production boundary

Goldeneye production builds use Cargo packages under `crates/`. Removing `tools/agent-bench/` removes benchmark runtime tooling without changing the Cargo package graph. Previous top-level benchmark runner paths directly under `tools/` no longer exist.
