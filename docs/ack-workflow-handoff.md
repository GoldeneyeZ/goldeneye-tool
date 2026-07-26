# ACK over Goldeneye: operational handoff

## Objective

Make ACK the compact, reliable model-facing interface to Goldeneye. An agent
should be able to enter any indexed repository, discover the minimum code it
needs through one ACK route, edit with the normal workspace tools, and recover
from configuration or index-freshness problems without falling back to direct
Goldeneye MCP calls.

Phase 1 remains read/discovery focused. Do not add or depend on `ack elect`.
The broader feature mapping remains in `docs/ack-feature-handoff.md`; this file
tracks the operational work required before ACK can replace direct MCP in agent
workflows and benchmarks.

## Verified state

The installed `ack` command resolves to:

```text
C:\Users\Zacha\AppData\Local\pnpm\ack.ps1
  -> C:\Users\Zacha\IdeaProjects\agent-context-kernel\dist\main.js
```

The implemented command surface agrees with the current `goldeneye-ack` skill:

```text
ack search, symbol, inspect, get, callers, callees, arch, status, index
```

The compact one-route workflow works when ACK is explicitly connected to a
fresh Goldeneye index. It was verified with:

```powershell
$env:ACK_MCP_COMMAND = 'D:\Dev\IdeaProjects\goldeneye-tool\target\release\goldeneye.exe'
$env:ACK_PROJECT = 'D-Dev-IdeaProjects-goldeneye-tool'
ack index D:\Dev\IdeaProjects\goldeneye-tool
ack status
```

The refreshed audit index reported generation 3, 404 files, 9,541 nodes, and
16,880 edges. After refresh, `search`, `get`, `inspect`, `callees`, `status`,
and `arch` returned current paths and usable compact output.

Two blocking failures were reproduced before that setup:

1. `ACK_MCP_COMMAND` pointed to `codebase-memory-mcp`, not Goldeneye, and
   `ACK_PROJECT` was unset. `ack status` failed with `ACK_PROJECT is required
   for codebase-memory commands`.
2. Goldeneye reported the project as `ready`, but ACK results referenced files
   removed by a refactor. `ack get` and `ack inspect` then failed with an
   `os error 3` source-file error. Running `ack index ...` repaired the route.

## Priority work

### P0: make backend and project selection dependable

Owner: `C:\Users\Zacha\IdeaProjects\agent-context-kernel`

- Stop defaulting this installation to `codebase-memory-mcp`. Resolve a stable
  installed Goldeneye executable or a stable wrapper; do not depend on a
  repository-local `target\release\goldeneye.exe`.
- Remove the normal need to set `ACK_PROJECT` manually. Resolve the project
  from the current repository root, or persist the canonical project returned
  by `ack index`. Explicit project selection should still override discovery.
- Make `ack index <repo>` the bootstrap command when no project is configured.
  Its success output must include the canonical project ID and enough
  information for subsequent commands to use it.
- Distinguish configuration errors: backend executable missing, transport
  startup failure, project unset, project unknown, and repository not indexed.
  Each error should suggest the single next command that repairs it.
- Cover Windows paths containing spaces and add PowerShell and Bash setup
  examples to the ACK README.

Definition of done: a clean shell in an indexed repository can run `ack
status` without manually exporting `ACK_MCP_COMMAND` or `ACK_PROJECT`.

### P0: detect and recover stale indexes

Owners: ACK CLI first; Goldeneye status contract if backend evidence is needed.

- Extend status/freshness evidence beyond database state `ready`. At minimum,
  compare the indexed repository root and indexed file metadata with the
  working tree well enough to detect deleted, renamed, or changed source files.
- Map missing source paths and stale source hashes to a dedicated stale-index
  error instead of exposing a generic MCP failure.
- On a stale `get` or `inspect`, print an actionable `ack index <repo>` hint.
  An optional automatic recovery may reindex and retry the original ACK route
  once; never loop and never repeat the query through direct MCP.
- Add regression cases for deletion, rename, content change, and project-root
  relocation.

Definition of done: moving an indexed source file changes `ack status` to
stale, and one documented recovery action makes the original route succeed.

### P0: align agent instructions with the ACK-only contract

Owners:

- `C:\Users\Zacha\.codex\skills\goldeneye-ack\SKILL.md`
- `C:\Users\Zacha\.codex\AGENTS.md`

The global instructions currently contain two incompatible workflows: the
older Goldeneye section requires direct MCP preflight and gives direct MCP
priority/examples, while the ACK section says ACK is the sole model-facing
surface.

Replace the old preflight with:

```text
1. Use `ack status` before code discovery.
2. If missing or stale, run `ack index <repository>`.
3. Choose one ACK discovery route from the skill.
4. Use direct Goldeneye MCP only when ACK is unavailable or fails.
5. Do not repeat a successful or failed query through both surfaces.
```

Extend the skill with:

- backend/project prerequisites and bootstrap behavior;
- `status` and `index` as preflight/recovery routes, separate from discovery;
- the stale-index recovery rule and one-retry limit;
- regex examples such as `--file '\\.tsx?$'` and an explicit warning that
  `--file` is currently a regex, not a glob;
- confirmation that normal edit/write tools remain allowed after ACK discovery.

Keep the current route-selection table and compact stopping rule. In
particular, do not search before a broad `inspect`, and do not run `inspect`
then `get` when exact source was required from the start.

### P1: harden compact CLI ergonomics

Owner: `C:\Users\Zacha\IdeaProjects\agent-context-kernel`

- Either normalize simple file globs (`*.ts`, `*.test.ts`) or reject them with
  a concrete regex replacement. Model runs repeatedly supplied globs where the
  backend expected regex.
- Clamp or hide backend bounds such as syntax preview limits so an otherwise
  valid model request cannot fail solely for exceeding a small server maximum.
- Give stable exit codes or machine-readable error kinds for: no result,
  ambiguous result, stale index, missing project, invalid filter, and backend
  failure.
- Keep output bounded and stable. Preserve qualified names, paths, labels,
  result totals/truncation hints, and the exact selector needed by the next
  route. Avoid emitting full source unless `get` was chosen.
- Add command-level latency and output-byte measurements behind a diagnostic
  flag so benchmark attribution does not require parsing human output.

### P1: add an ACK agent benchmark lane

Owner: `D:\Dev\IdeaProjects\goldeneye-tool\tools\agent-bench`

The existing Goldeneye-versus-vanilla task benchmark exercises direct
Goldeneye MCP, not ACK. Its combined six-sample warm results were:

| Metric | Direct Goldeneye MCP | Vanilla | Difference |
| --- | ---: | ---: | ---: |
| Agent wall-clock p50 | 180.59 s | 174.55 s | +3.5% |
| End-to-end p50 | 191.91 s | 174.55 s | +9.9% |
| Total tokens | 893,597 | 403,591 | +121.4% |
| Uncached input tokens | 42,314 | 26,652 | +58.8% |
| Tool calls | 22.5 | 10.5 | +114.3% |

ACK is intended to remove much of this discovery and context overhead. Add a
third engine kind, `ack`, with these rules:

- expose ACK CLI discovery only, explicitly bound to Goldeneye;
- reject direct Goldeneye MCP discovery in the ACK lane;
- prohibit direct reads of TypeScript, TSX, and Rust source in the ACK lane;
- allow normal workspace edit/write tools after discovery;
- treat ACK CLI calls as expected behavior rather than a protocol violation;
- record ACK command, route, duration, exit status, stdout bytes, and stderr
  bytes in addition to the existing agent metrics.

Before trusting the comparison, fix two known harness false positives:

1. The direct-source-read detector splits a piped PowerShell command and can
   misclassify a permitted `package.json` read because it loses the extension.
2. The frontend grader inspects only the existing `fuzzy.test.ts` even though
   the task permits adding a new focused test file.

Run at least three cold and three warm samples for ACK and vanilla on the same
frontend task and report completion, correctness, agent wall time, end-to-end
time, total and uncached tokens, tool calls, ACK calls, and ACK output bytes.
Retain raw result JSON under `target/agent-bench`.

Initial success targets:

- identical task pass rate to vanilla;
- median 3-4 ACK discovery calls per successful task;
- warm end-to-end p50 no slower than vanilla;
- total and uncached tokens no higher than vanilla, or a clearly justified
  quality gain that outweighs the difference;
- materially lower tokens and tool calls than the direct-MCP Goldeneye lane.

### P2: package and continuously verify the workflow

Owners: both repositories.

- Turn `tools/ack-acceptance.mjs` into the authoritative smoke suite for the
  Phase-1 routes and run it against the installed ACK shim plus the intended
  Goldeneye binary, not an accidentally stale development process.
- Cover bootstrap, status, search-to-get, direct get, broad inspect, callers,
  callees, architecture, stale recovery, regex validation, and bounded output.
- Assert that acceptance traces contain no direct Goldeneye MCP invocation.
- Document the installed version of ACK and the Goldeneye protocol/version it
  was tested against.
- Add an update/install command that atomically replaces the ACK shim target
  and validates `ack --help` plus `ack status` afterward.

## Recommended implementation sequence

1. Stable Goldeneye binding and cwd-based project resolution.
2. Freshness detection, error mapping, and one-step recovery.
3. Global AGENTS and `goldeneye-ack` skill cleanup.
4. Filter/error/output hardening.
5. Benchmark harness fixes and ACK lane.
6. Three-by-three ACK-versus-vanilla run and analysis.
7. Package the acceptance suite into installation/update verification.

Keep these as focused commits so configuration, freshness, prompting, and
benchmark effects can be measured independently.

## Final acceptance checklist

- [ ] `ack` launches a stable Goldeneye backend by default.
- [ ] A repository can bootstrap with `ack index <repo>` without pre-setting a
      project ID.
- [ ] `ack status` accurately distinguishes ready, stale, missing, and failed.
- [ ] All Phase-1 routes work against current paths after bootstrap.
- [ ] A stale source produces one actionable recovery path and succeeds after
      reindexing.
- [ ] Global instructions never require direct MCP while ACK is healthy.
- [ ] The ACK lane performs no direct MCP discovery or direct TS/TSX/Rust reads.
- [ ] ACK matches vanilla correctness and meets the agreed performance targets.
- [ ] `ack elect` remains absent from Phase 1.

## Evidence and related files

- `docs/ack-feature-handoff.md`: broader capability and protocol handoff.
- `docs/ack-acceptance.md`: current Phase-1 acceptance baseline.
- `tools/ack-acceptance.mjs`: existing ACK acceptance harness.
- `target/agent-bench/terax-goldeneye-vs-vanilla.json`: first agent-task run.
- `target/agent-bench/terax-goldeneye-vs-vanilla-rerun.json`: rerun.
- `tools/agent-bench/core.mjs`: engine policy, telemetry, and source-read checks.
- `tools/agent-bench/graders/terax-fuzzy-diacritics.mjs`: frontend task grader.
- `C:\Users\Zacha\IdeaProjects\agent-context-kernel\README.md`: current ACK
  configuration contract.
- `C:\Users\Zacha\IdeaProjects\agent-context-kernel\src\cli\runCli.ts`: current
  project-selection enforcement.
