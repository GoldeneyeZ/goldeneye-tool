# GCAL Token-Efficiency Hardening Design

## Goal

Reduce GCAL's total model input per successful code-discovery task by removing unbounded output, repairing relationship lookup, and ensuring agents use GCAL as one mutually exclusive discovery surface instead of layering GCAL over direct MCP calls.

## Evidence

The benchmark processed 29.25M input tokens with GCAL and 12.16M without it. Fresh input increased 53.9%, while cached input increased 143.2%. This pattern indicates repeated model/tool turns over a growing transcript, not a cache-quality improvement.

Three repository behaviors materially amplify those turns:

- `arch` forwards the full architecture response. On the benchmark project, the response was roughly 89k characters and its `file_tree` contributed about 75k characters.
- The live MCP trace contract returns `callers` or `callees` arrays with `qualified_name` and `hop`; GCAL only reads the legacy `paths` array. Empty relationship output encourages retries and fallback discovery.
- Installed instructions can mandate both direct codebase-memory MCP tools and GCAL. The detailed GCAL skill also repeats much of the global block and encourages serial `search -> inspect -> get` waterfalls.

## Scope

This first pass changes existing Phase 1 primitives only:

- normalize current and legacy trace payloads;
- compact relationship rows;
- bound architecture, search, and standalone trace output;
- reduce default trace depth;
- make GCAL the sole model-facing discovery route while it is available;
- migrate the legacy direct-MCP instruction block during Windows installation;
- align README and generated workflow assets with the new routing.

This pass does not add `gcal elect`, multi-symbol source batching, model telemetry, or a live MCP dependency to normal tests. Those remain follow-up work after a controlled benchmark confirms the first-pass attribution.

## Runtime Design

### Trace normalization

The adapter accepts both response families:

- current: `callers: [{ qualified_name, hop, ... }]` or `callees: [...]`;
- legacy: `paths: [{ caller, callee, file_path, start_line, ... }]`.

The adapter converts either shape into `TraceEdge`. `TraceEdge` gains nullable `hop` metadata. For current rows, direction determines the selected and related endpoints: inbound rows map the related symbol to the source, while outbound rows map it to the target.

Standalone relationship rows become:

```text
related_qualified_name<TAB>hop<TAB>file<TAB>line
```

This removes the duplicate related/source/target qualified names currently printed on every row.

### Bounded CLI defaults

- `search` and `symbol` default to 5 candidates instead of 20.
- `callers` and `callees` default to depth 1 instead of 3.
- Standalone traces accept `--limit`, defaulting to 20 rows.
- When a trace is truncated, stdout remains headerless relationship rows and stderr receives one concise continuation message with the total and the required larger limit.

Explicit user values continue to override all defaults. Inspect remains depth 1 and keeps its existing high-caller hint behavior.

### Architecture projection

`arch` requests only these high-signal aspects:

- languages;
- packages;
- entry points;
- hotspots;
- boundaries;
- layers;
- clusters.

The adapter also projects the response onto those fields plus project and graph totals, so server drift cannot reintroduce `file_tree`, routes, or other unbounded sections. Array sections are capped at 20 entries. Output remains compact one-line JSON.

## Workflow Design

The installed `AGENTS.md` block becomes a short routing override. While GCAL works, agents do not call raw codebase-memory MCP tools for the same discovery task. Raw MCP is a fallback only after GCAL is unavailable or fails, and the same query is not retried through both surfaces.

The detailed skill uses mutually exclusive routes:

| Need                                            | Route                                       |
| ----------------------------------------------- | ------------------------------------------- |
| Exact qualified name and source body            | `gcal get` directly                          |
| Unknown symbol and source body                  | one `search` or `symbol`, then `get`        |
| Metadata determines whether source is necessary | `inspect` directly                          |
| Relationship or impact evidence                 | `callers` or `callees --depth 1 --limit 20` |
| High-level project shape                        | `arch` once                                 |

The skill explicitly prohibits `search -> inspect` duplication and `inspect -> get` when the task already requires source. It also adds a stop rule: once the evidence answers the task, do not continue discovery.

The Windows installer removes the known legacy `codebase-memory-mcp` managed instruction block before installing GCAL's managed block. It does not remove the MCP server or prevent direct fallback access; it only removes contradictory model instructions.

## Testing

All MCP behavior remains fixture-based.

- Adapter tests cover current inbound/outbound arrays and retain one legacy `paths` compatibility case.
- Formatter tests protect the compact relationship row contract.
- CLI tests protect new defaults, explicit overrides, row limits, and the stderr truncation message.
- Architecture tests verify requested aspects, omitted `file_tree`, and section caps.
- Workflow tests compare generated assets with committed assets and assert the mutually exclusive routing rules.
- Installer tests assert the legacy managed block migration.

Completion requires `pnpm check` with no live MCP endpoint.

## Benchmark Follow-up

After release, rerun paired tasks with the same repository commit, model, prompt, tool list, warmed index, and acceptance test. Compare baseline discovery, GCAL without workflow guidance, and GCAL with guidance. Record request count, per-command output bytes, duplicate symbol retrieval, fresh input, cached input, compactions, and successful completion. The optimization target is total weighted tokens per accepted result, not cache-hit percentage.
