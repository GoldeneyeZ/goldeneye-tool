---
name: goldeneye-code-agent-layer
description: Use GCAL over Goldeneye for project initialization, compact code discovery, trusted JavaScript workflows, exact source retrieval, call tracing, architecture checks, and index status.
---

# Goldeneye Code Agent Layer

## Choose One Route

| Need                                         | Route                                                                                                                 |
| -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| Exact qualified name and source needed       | Run `gcal get <qualified-name>` directly.                                                                             |
| Multiple exact sources needed                | Run one `gcal get <qualified-name...>`.                                                                               |
| Unknown symbol and source needed             | Run `gcal search <query> --snippets`, then select only relevant evidence.                                             |
| Multiple unknown symbols                     | Run one `gcal search <query-1> --query <query-2> ... --snippets <n>`, then fetch only selected results needing more.  |
| Metadata determines whether source is needed | Run `gcal inspect` directly. Do not run `search` before `inspect`; broad inspect already searches.                    |
| Relationship or impact evidence              | Run `gcal callers` or `gcal callees` with `--depth 1 --limit 20`.                                                     |
| Adaptive or repeated dependent discovery     | Run trusted code with `gcal workflow --file <path>` or `gcal workflow --js <code>`.                                   |
| Exact symbol plus several evidence types     | Use one JavaScript workflow and parallelize independent operations with `Promise.all`.                               |
| High-level project shape                     | Run `gcal arch` once.                                                                                                 |
| Project not registered                       | Run `gcal init` once from the project root.                                                                           |

Do not run `inspect` and then `get` when the task already requires source. Stop discovery once the task has enough evidence.

## Batch and Call Budget

Batching is allowed, not required. Prefer the smallest batch that directly advances
the task. Keep batches at five items or fewer. Exceed five only when every item is
directly relevant to the current task; never pad a batch with speculative work.

A normal unknown-source lookup uses one `gcal search --snippets` invocation. Use a
JavaScript workflow when later searches, selections, source reads, or traces depend
on earlier results, or when the task needs a bounded loop. Combine independent
queries or qualified names before calling GCAL. Relationship and architecture
lookups normally use one invocation.
Do not run `gcal status` or `gcal --help` unless blocked by project state or syntax.

Batch `gcal get <qualified-name...>` and `gcal search --snippets` already use bounded
direct chunks. Do not repeat those results with separate unflagged `gcal get` calls
unless the task earns more source.

## Trusted JavaScript Workflow

`gcal workflow` executes a true async JavaScript body in one worker. Supply exactly
one of `--js <code>` or `--file <path>`. The body can loop, branch, use
`Promise.all`, and call `gcal.search`, `gcal.select`, `gcal.source` (`gcal.get` is an
alias), `gcal.trySource`, `gcal.callers`, `gcal.callees`, `gcal.tryCallers`, and
`gcal.tryCallees`. `gcal.trySource`
returns `{ ok: true, ...sourceFields }` or `{ ok: false, error }`, so one source
miss does not reject a batch. Read successful source text from `.source` directly.
Return a JSON-serializable value.
Prefer `--file` for multi-line programs; reserve inline `--js` for short bodies to
avoid shell-quoting corruption.
Use a targeted `filePattern` search option and deduplicate candidates by `filePath`
when a broad query spans code, tests, docs, or resource files. Search results may
include project/module candidates without source; use `gcal.trySource` for batches.
`filePattern` accepts regex or common `*`/`**` glob spelling.
Use `gcal.tryCallers` or `gcal.tryCallees` for optional traces; successful results
are `{ ok: true, edges }` and misses are `{ ok: false, error }`.

```js
const hits = await gcal.search("authentication", { limit: 10 });
const evidence = [];
for (const hit of hits) {
  const source = await gcal.trySource(hit.qualifiedName);
  if (!source.ok) continue;
  if (!source.source.includes("token")) continue;
  const callers = await gcal.callers(hit.qualifiedName, { depth: 1, limit: 20 });
  evidence.push({ hit, source, callers });
}
return evidence;
```

Keep defaults unless the task requires more: 32 backend calls and 30 seconds.
Flags can raise these to hard caps of 128 calls and 120 seconds. Operation caps are
20 search candidates, one 8 KiB source chunk, trace depth four, and 50 trace rows.
Returned output is capped at 48 KiB; captured console output is capped at 8 KiB.

This is trusted Node.js, not a security sandbox. Worker limits protect liveness but
do not block filesystem, network, environment, or secret access. Execute only code
you trust. Keep loop termination explicit even though the wall-clock timeout exists.

## Bounded Source Retrieval

Small single-symbol `gcal get <qualified-name>` returns exact source. If GCAL
returns a `snippet-too-large` manifest instead, use its exact continuation
command:

```bash
gcal get <qualified-name> --chunk 1 --expected-source-sha <source-sha256>
```

Continue with increasing 1-based `--chunk` values while the returned metadata
shows another chunk. Keep the same manifest SHA for every chunk so Goldeneye
fails closed if indexed source changes. Never omit or invent the SHA after a
manifest supplies one.

## Project Selection

Run `gcal init` once from a project root to index it and register its Goldeneye project name in `$HOME/.gcal/projects.json`.

For project-backed commands, GCAL resolves the current directory against the most specific registered ancestor. `GCAL_PROJECT` is an optional explicit override for CI, scripts, or commands outside a registered project.

GCAL stores state in `$HOME/.gcal` by default. Set `GCAL_HOME` to an isolated absolute state directory for benchmarks or tests.

Use `gcal init [repoPath]` when GCAL reports no registered project. `gcal index` only indexes and does not update the local registry. Never invent or derive a Goldeneye project name from a path.

## Daemon Reuse

Normal Goldeneye calls use an on-demand local GCAL daemon and reuse one backend
session per active project. The daemon exits after 10 minutes idle by default.
Set `GCAL_DAEMON_IDLE=10m` or configure `daemon.idleTimeout` in
`$HOME/.gcal/config.json`. Set `GCAL_DAEMON=off` only when direct per-command
Goldeneye startup is required. Benchmark mode always bypasses the daemon.

Daemon reuse removes repeated Goldeneye startup but not agent or CLI round trips.
Keep using relevant batches and the call budget above.

## Backend Selection

Goldeneye is GCAL's default backend. Normal use leaves `GCAL_BACKEND` unset or sets it to `goldeneye`. `GCAL_GOLDENEYE_COMMAND` may contain a full Goldeneye executable path when `goldeneye` is not on `PATH`.

Use `GCAL_BACKEND=benchmark` only for explicit compatibility measurements. Benchmark mode requires `GCAL_MCP_COMMAND` and may use `GCAL_MCP_URL`. Never let benchmark configuration leak into normal Goldeneye use.

## Literal-safe Search

`gcal search` treats input as literal-safe. Use `A|B` for alternatives; GCAL merges branches stably, deduplicates by qualified name, and applies one global limit. Java annotations such as `@SpringBootTest` are normalized. A leading wildcard such as `*Test` routes to an escaped suffix symbol match; use `gcal symbol` for other regex searches.

## Raw Text Search

Use raw text search when the target is a literal, configuration value, non-code file, generated asset, or documentation text, or when GCAL is unavailable, fails, or returns clearly weak results.

Do not implement or rely on `gcal elect` in Phase 1.
