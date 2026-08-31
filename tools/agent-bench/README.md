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
```

Direct entrypoints:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs <arguments>
node tools/agent-bench/bin/benchmark-competitors.mjs <arguments>
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

One-shot attempts are standalone and unqualified. They skip agent-side build, compile, test, lint, and check commands; discovery follows the selected lane strategy and budget. A missing or stale warm GCAL snapshot is refreshed locally without a smoke/model run. By default, output is isolated at `target/agent-bench/<task-id>/one-shot/<attempt-id>/report.json`; use `--attempt-id <id>` for a stable name or `--out <path>` for an explicit report. Canonical scored reports are never read or merged.

## Layout

- `bin/`: benchmark entrypoints
- `configs/`: benchmark configurations
- `graders/`: grading code and injected fixtures
- `tasks/`: agent task prompts
- root modules: shared harness, timing, provenance, qualification, reporting, and snapshot logic

## Inputs and outputs

Entrypoint arguments and environment variables remain defined by each runner and config. Generated benchmark evidence belongs under configured output directories, normally `target/agent-bench/`; generated evidence is not production runtime input.

## Production boundary

Goldeneye production builds use Cargo packages under `crates/`. Removing `tools/agent-bench/` removes benchmark runtime tooling without changing the Cargo package graph. Previous top-level benchmark runner paths directly under `tools/` no longer exist.
