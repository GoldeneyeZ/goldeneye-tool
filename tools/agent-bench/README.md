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

## Layout

- `bin/`: benchmark entrypoints
- `configs/`: benchmark configurations
- `graders/`: grading code and injected fixtures
- `tasks/`: agent task prompts
- root modules: shared harness, timing, provenance, qualification, reporting, and snapshot logic

## Inputs and outputs

Entrypoint arguments and environment variables remain defined by each runner and config. Generated benchmark evidence belongs under configured output directories, normally `target/agent-bench/`; generated evidence is not production runtime input.

## Production boundary

Goldeneye production builds use Cargo packages under `crates/`. Removing `tools/agent-bench/` removes benchmark runtime tooling without changing the Cargo package graph. Legacy paths `tools/agent-bench/bin/benchmark-agent-tasks.mjs` and `tools/agent-bench/bin/benchmark-competitors.mjs` no longer exist.
