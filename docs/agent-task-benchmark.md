# Paired agent task benchmark

`tools/agent-bench/bin/benchmark-agent-tasks.mjs` compares Goldeneye,
`codebase-memory-mcp`, and vanilla Codex on actual code changes, not isolated
search calls. Each lane receives the same repository commit, task prompt, Codex
model, reasoning effort, sandbox, command-line tools, and held-out grader. MCP
lanes differ only by the executable registered as `codebase_memory_mcp`;
vanilla registers no MCP server and uses ordinary shell/file discovery.

The runner uses detached Git worktrees and executes runs sequentially in a
seeded random order. It disables the user's Codex configuration and repository
rules, then supplies a common benchmark prompt. Invoking the global `ack` CLI is
forbidden and recorded as a protocol violation so it cannot silently route both
lanes through the same backend. MCP lanes must also use their assigned graph
engine to discover and read `.ts`, `.tsx`, and `.rs` source. Direct source reads
through shell search/read commands are recorded as protocol violations; vanilla
remains unrestricted. `git diff`, editing, and test/lint/typecheck commands are
allowed.

The Terax configuration enables Codex's non-interactive full-access mode because
custom MCP approvals and writes are otherwise rejected by current headless Codex
on Windows. Runs operate only in disposable worktrees and the prompt forbids
writes outside them, but this is a process-level trust boundary rather than an OS
sandbox. Run the harness only with task prompts and MCP executables you trust.

## What is measured

- Grader success rate. Failed runs are never included in latency or token
  percentiles.
- Agent wall time, from `codex exec` spawn through exit. Build, warm pre-index,
  and held-out grading time are reported separately.
- Input, cached-input, output, and reasoning-output tokens from Codex JSONL
  telemetry.
- MCP, shell-command, and total tool calls when exposed by Codex telemetry.
- MCP and indexing failures. A failed `index_repository` attempt invalidates the
  run even when the agent later finishes via raw search.
- JSONL event bytes and patch size, files, insertions, and deletions.
- Cold and warm cache conditions independently.

Cold MCP runs start with an empty engine cache and include any indexing initiated
by the agent. Warm MCP runs pre-index the untouched worktree before starting the
agent; that duration is recorded as `preindex_ms` and excluded from completion
time. Vanilla runs once per repetition with `cache_mode="none"`; duplicating it
across cold/warm would measure the same condition twice. Every run gets isolated
state, so repetitions do not leak into one another.

## Terax pilot

The checked-in pilot is pinned to Terax commit
`076d70f638ae7bff452e927d90dd34bceae079f8`. It asks the agent to make the
TypeScript command-palette fuzzy matcher accent-insensitive while preserving its
existing ranking behavior. A held-out Vitest suite checks precomposed and
decomposed Unicode accents, regressions, and the public API. No Rust source or
task-level Rust compilation is involved.

Validate the matrix without running agents:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/terax.config.json `
  --dry-run
```

Run a cheap paired pilot first:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/terax.config.json `
  --repetitions 1 `
  --cache-modes cold
```

Run the full configured experiment (15 agent runs: two MCP engines × two cache
conditions × three repetitions, plus three vanilla runs):

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/terax.config.json
```

The default artifact is `target/agent-bench/terax-report.json`; per-run prompts,
JSONL telemetry, stderr, final messages, patches, status, and grader logs are
stored below `target/agent-bench/runs/`.

Disposable Git worktrees and caches live in short `.gab`/`.gab-cache` sibling
directories beside the target repository. Hashed lane names keep derived SQLite
paths below Windows path limits. They remain outside Goldeneye's Cargo workspace
but under a shared allowed root with Terax's main Git directory, preventing
Cargo/pnpm parent-directory contamination while allowing MCP engines to follow
Git-worktree metadata. When the source checkout has `node_modules`, the runner
links it into each disposable worktree so frontend verification does not require
dependency installation.

Use `--skip-build` only when the release Goldeneye binary already reflects the
source being evaluated. Useful selectors are `--task`, `--engine`,
`--cache-modes`, `--repetitions`, `--seed`, `--model`, `--reasoning`, and
`--out`. `--keep-worktrees` and `--keep-caches` are debugging aids and should not
be used for normal measurements.

## Adding tasks

Add a prompt and an external grader, then append a task object to the JSON
configuration. Graders receive `{worktree}`, `{repo}`, `{taskDir}`, and `{runDir}`
placeholders. A task should describe observable behavior without revealing the
grader, cover code discovery plus a meaningful edit, and be pinned to a commit
where the requested behavior is absent.

Treat completion-time differences as evidence only when success rates are
comparable. Local process scheduling, filesystem cache, model variance, and
service load remain sources of noise; keep sequential execution, use at least
three repetitions, and retain the randomized order and raw run artifacts.
