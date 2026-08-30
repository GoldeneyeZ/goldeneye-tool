# Goldeneye stability round 1

Date: 2026-07-22

## Scope

Stabilize direct Goldeneye MCP availability before introducing the GCAL benchmark lane.

The Abyssal Zenith baseline had one genuine warm-run failure:

- result: no patch; held-out grader failed
- agent wall time: 36.9 seconds
- tool calls: 1 (`git status`)
- MCP calls: 0
- final message: assigned Goldeneye tools were unavailable
- Codex stderr repeated four tool-planning warnings for `delete_project`

## Root cause

`delete_project_tool` passed an already-complete `project_only` object schema back through `object_schema`. This produced a malformed nested `properties` value:

```text
properties: {
  additionalProperties: false,
  properties: { project: ... },
  required: ["project"],
  type: "object"
}
```

Codex could not deserialize that tool definition and emitted:

```text
Skipping deferred MCP tool `mcp__codebase_memory_mcpdelete_project`: failed to build tool spec: invalid length 1, expected struct JsonSchema with 14 elements
```

The affected agent then treated the Goldeneye tool inventory as unavailable and correctly refused prohibited raw Java reads.

## Fix

- `delete_project_tool` now clones the valid shared `project_only` schema, matching `index_status` and `get_graph_schema`.
- MCP registry test asserts exact schema equality between `delete_project` and `index_status`.
- Frozen foundation contract fixture records the corrected schema.
- Release binary rebuilt with the corrected MCP registry.

## Repeated-agent validation

Command:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/abyssal-zenith-goldeneye-serena-vanilla.config.json `
  --engine goldeneye `
  --cache-modes warm `
  --repetitions 3 `
  --task abyssal-public-endpoints `
  --skip-build `
  --out target/agent-bench/abyssal-zenith-goldeneye-stability-after-schema.json
```

Results:

| Metric | Before | After |
|---|---:|---:|
| Warm correctness | 2/3 | 3/3 |
| Inventory-blocked exits | 1/3 | 0/3 |
| Deferred MCP schema warnings in failed run | 4 | 0 across all 3 runs |
| MCP usage | failed run: 0 calls | every run: 13–18 calls |
| Agent wall p50 | 103.6s over successful runs | 133.8s |
| Verified E2E p50 | 117.0s over successful runs | 165.7s |
| Total tokens p50 | 476,470 | 856,875 |

Reliability improved in this sample; performance did not. Three repetitions remain too small for a general reliability claim.

## Remaining instability

Every post-fix run completed correctly, but successful runs still had 2–3 recoverable MCP failures:

- `search_graph.file_pattern`: agents supplied glob forms such as `*Test.java` and `*.java`; tool requires regex.
- `get_code_snippet`: simple class names were ambiguous with same-named constructor nodes.
- `search_code`: one agent combined regex-like `path_filter` and `file_pattern` values rejected by path validation.

These are tool ergonomics and GCAL-routing issues, not MCP startup failures. Next stability round should eliminate them through GCAL's single command path and clearer query contracts before comparing tokens or latency.

## Verification

- MCP registry regression test: passed
- `cargo test -p goldeneye-mcp`: 50 passed
- frozen foundation contract: passed
- `cargo test --workspace`: passed
- `cargo clippy --workspace --all-targets -- -D warnings`: passed
- `cargo fmt --all --check`: passed
- release Goldeneye build: passed
- three warm held-out Java agent graders: passed

## Artifact

`target/agent-bench/abyssal-zenith-goldeneye-stability-after-schema.json`
