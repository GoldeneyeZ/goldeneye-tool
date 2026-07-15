# ACK feature handoff

This document hands the post-Phase-1 Goldeneye capabilities to ACK. The ACK
baseline is commit `5bc63b4`, where the Rust-only acceptance harness covered
`ack search`, `symbol`, `inspect`, `get`, `callers`, `callees`, `arch`,
`status`, and `index`. The implementation described here is commit `14cdb59`.

The exact protocol contract is `tests/fixtures/mcp/foundation.expected.jsonl`.
Use that fixture and HEAD source as the authority; do not infer schemas from a
possibly stale running development server. Phase 1 still excludes `ack elect`.

## Scope and baseline

The MCP registry grew from 10 to 21 tools. Eleven tool names are new:

```text
delete_project
search_code
inspect_syntax
create_file
replace_node
delete_node
insert_before_node
insert_after_node
detect_changes
manage_adr
ingest_traces
```

Existing contracts also changed:

- `index_repository` now defaults to `full` instead of `fast`, supports
  `moderate`, `fast`, and `cross-repo-intelligence`, and accepts `name`,
  `persistence`, and `target_projects`.
- `search_graph` accepts `semantic_query`, an array whose keywords are scored
  independently using the minimum cosine score.
- `query_graph` keeps the same bounded read-only envelope but now supports a
  much larger Cypher subset: expressions and `CASE`, `MATCH`/`WITH` pipelines,
  chained patterns, `UNION`, `UNWIND`, and bounded traversal reporting.
- Indexing now refreshes Git history and semantic artifacts. `fast` clears the
  semantic index; `moderate` and `full` rebuild it.

## Recommended ACK surface

| Goldeneye capability | Suggested ACK surface | Priority | Status |
| --- | --- | --- | --- |
| Expanded index modes | Extend `ack index` with `--mode` and `--name` | P0 | Ready with name-collision preflight |
| Graph-augmented text search | `ack code <pattern>` | P0 | Ready; default to `compact` |
| Semantic symbol search | `ack search --semantic <keyword>...` | P0 | Ready; environment toggles are not wired and watcher refresh clears vectors |
| Git change impact | `ack changes` | P0 | Ready |
| Bounded syntax inspection | `ack syntax <path>` | P0 | Ready; required before node edits |
| Structural mutations | `ack create`, `replace`, `delete-node`, `insert-before`, `insert-after` | P0 | Ready with locator and operation-ID rules |
| Project administration | `ack projects`, `ack project delete`, `ack schema` | P1 | Runtime works; `delete_project` advertised schema is malformed |
| Read-only Cypher | `ack query <cypher>` | P1 | Ready; expose row bounds |
| Cross-project intelligence | `ack index --mode cross-repo-intelligence` | P1 | Partial; target filtering is not implemented |
| ADR storage | `ack adr get|update|sections` | P1 | Partial; requested section filtering is ignored |
| Trace ingestion | `ack traces ingest` | P2/experimental | Storage works; graph-edge materialization does not |
| Local graph UI | `ack ui` | P2/local only | Ready only on loopback through the Goldeneye binary |
| Watcher controls | None yet | Deferred | Watcher starts automatically; no MCP status/control contract |
| Artifact sharing | `ack artifact ...`, `ack index --persist` | Blocked | Unsafe with the shared multi-project database |
| Full grammar runtime | Runtime/provider selection | Blocked | Full pack exists but shipped services use only core grammars |

ACK should retain its compact-first behavior:

- Prefer `structuredContent`; fall back to decoding `result.content[0].text`.
- Keep `search_code` in `compact` mode unless `--full` is explicit.
- Preserve `total_grep_matches`, `total_results`, `has_more`, cursors, and
  truncation hints so agents know when context is incomplete.
- Render short locator handles, but retain the complete opaque locator for the
  next mutation. Never reconstruct a locator from a line number.
- Read mutation content from stdin or a file. Avoid large command arguments.
- Generate one unique `operation_id` per logical mutation and reuse it for a
  transport retry; do not generate a second ID for the same intended write.
- Print compact mutation summaries by default and reserve full diagnostics,
  refreshed locators, and graph IDs for `--json` or an explicit verbose mode.

## New feature inventory

### Structural editing

Goldeneye now supports durable, syntax-aware file creation and node mutation:

- `inspect_syntax` returns a bounded named-node tree, diagnostics, generation,
  file hash, and guarded locators.
- `create_file` creates one authorized project-relative file and never
  overwrites an existing destination.
- `replace_node`, `delete_node`, `insert_before_node`, and
  `insert_after_node` operate on exactly one locator-selected named node.
- Locators bind project, relative path, language, grammar identity and ABI,
  file hash, generation, ancestor path, node kind, source span, and content
  hash. A stale locator never writes.
- Writes are journaled, atomic, parse-policy checked, and followed by graph
  refresh. Startup reconciles incomplete journal entries before serving work.
- Results include old/new hashes, a compact diff, syntax and graph changes,
  generation, diagnostics, and approximate context size.

### Indexing and query

- `full`: all discovered files, similarity/semantic artifacts.
- `moderate`: filtered discovery, similarity/semantic artifacts.
- `fast`: filtered discovery, no similarity/semantic artifacts.
- `cross-repo-intelligence`: skips extraction and rebuilds derived links.
- Git history adds file history and co-change enrichment.
- Hybrid call resolution combines per-file and cross-file type-aware results.
- Enrichment adds routes, protocol calls, handlers, environment/config links,
  package links, and data-flow edges. Recognized service calls include HTTP,
  async brokers, gRPC, GraphQL, and tRPC.
- `search_code` runs text search, deduplicates hits into containing functions,
  and ranks definitions before tests. Modes are `compact`, `full`, and
  `files`; `full` caps source windows at 60 lines.

### Cross-project intelligence

`goldeneye-crosslink` derives forward and reverse edges with remote project,
name, and file metadata:

```text
HTTP_CALLS    -> CROSS_HTTP_CALLS
ASYNC_CALLS   -> CROSS_ASYNC_CALLS
GRAPHQL_CALLS -> CROSS_GRAPHQL_CALLS
GRPC_CALLS    -> CROSS_GRPC_CALLS
TRPC_CALLS    -> CROSS_TRPC_CALLS
channel match -> CROSS_CHANNEL
```

Duplicate identities collapse. Each project is bounded to 100,000 derived
cross edges.

### Runtime services

- `goldeneye-http` serves the embedded UI, JSON-RPC, graph layouts, indexing
  jobs, project management, repository browsing, ADRs, health, processes, and
  logs from a bounded local HTTP server.
- `goldeneye-watcher` uses adaptive polling, Git/file baselines, retry,
  cancellation, wakeups, missing-root grace, and pruning. Watcher re-indexing
  uses `fast` mode and preserves a configured project-name override.
- `goldeneye-artifact` implements compressed SQLite snapshots, metadata,
  integrity verification, bounded decompression, and atomic installation.
- The standalone graph UI preserves project selection, graph search and
  filters, dead-code and missed-coverage views, density controls, stable 3D
  layouts, ADR editing, and process/log controls.

## MCP contracts

The current registry groups its 21 tools as follows:

```text
Index/meta:   index_repository, list_projects, delete_project,
              index_status, get_graph_schema
Search/query: search_graph, search_code, query_graph
Trace/source: trace_path, trace_call_path, get_code_snippet, get_architecture
Edit:         inspect_syntax, create_file, replace_node, delete_node,
              insert_before_node, insert_after_node
Compatibility: detect_changes, manage_adr, ingest_traces
```

The stdio server supports MCP protocol versions `2025-11-25`, `2025-06-18`,
`2025-03-26`, and `2024-11-05`. It exposes tools only: no resources, resource
templates, or prompts. `trace_call_path` remains an alias for `trace_path`.

### Added and expanded inputs

- `index_repository` requires `repo_path`. Optional fields are `mode=full`,
  `name`, `persistence=false`, and `target_projects[]`.
- `search_code` requires `project` and `pattern`. Optional fields are
  `mode=compact`, `regex=false`, `limit=10` (maximum 200), `context`,
  `file_pattern`, and `path_filter`.
- `query_graph` requires `project` and `query`; `max_rows` defaults to 200 and
  is bounded to 100,000.
- `inspect_syntax` requires `project` and `path`. Inspection defaults are
  `max_depth=4`, `max_nodes=200`, and `preview_chars=0`; optional filters are
  `byte_range` and `node_kinds[]`.
- `create_file` requires `operation_id`, `project`, `path`, `content`, and
  `expected_generation`. Optional fields are `language_id`, `create_parents`,
  and `parse_policy`.
- Node mutations require `operation_id` and the complete locator. Replace and
  insert operations also require `content`.
- Parse policies are `require_clean`, `no_additional_diagnostics`, and
  `allow_errors`. Creation defaults to `require_clean`; node edits default to
  `no_additional_diagnostics`.
- `detect_changes` requires `project`; optional fields are `scope`, `since`,
  `base_branch=main`, and `depth=2`. Output contains `changed_files`,
  `changed_count`, `impacted_symbols`, depth, and an optional hint.
- `manage_adr` requires `project`; optional fields are `mode`, `content`, and
  `sections[]`. Modes are `get`, `update`, and `sections`.
- `ingest_traces` requires `project` and `traces[]`. Trace objects accept
  `caller`, `callee`, and `count`; persistence is bounded to 1,024 records per
  batch and 1,024 bytes per endpoint.

Strict clients must account for one current schema defect: `delete_project`
advertises a double-wrapped object schema, but runtime dispatch expects the
flat payload `{ "project": "..." }`. Prefer fixing Goldeneye before relying on
schema-generated ACK commands; until then, special-case the flat payload.

## CLI and HTTP contracts

The Goldeneye binary has these direct modes:

```text
goldeneye
goldeneye --version | -V
goldeneye --help | -h
goldeneye ui [--bind <address>] [--base-path <path>]
goldeneye artifact export <repository> [fast|best]
goldeneye artifact import <repository>
goldeneye artifact exists <repository>
goldeneye artifact commit <repository>
```

No arguments starts stdio MCP. Artifact commands emit JSON; export defaults to
`fast`. UI prints its URL, defaults to `127.0.0.1:7878`, and has no base path.
The server caps headers at 32 KiB and bodies at 1 MiB.

The embedded UI uses:

| Method | Route | Purpose |
| --- | --- | --- |
| POST | `/rpc` | MCP-compatible `tools/call` |
| GET | `/api/layout` | Stable graph coordinates |
| GET | `/api/repo-info` | Branch and safe deep-link bases |
| POST | `/api/index` | Start an indexing job |
| GET | `/api/index-status` | List indexing jobs |
| GET | `/api/ui-config` | Language and issue URL |
| DELETE | `/api/project` | Delete a project |
| GET | `/api/browse` | Browse server-local directories |
| GET/POST | `/api/adr` | Read or save ADR text |
| GET | `/api/project-health` | Database health |
| GET | `/api/processes` | Goldeneye process list |
| GET | `/api/logs` | Recent process logs |
| POST | `/api/process-kill` | Terminate a selected process |

`ack ui` can only delegate to a locally configured Goldeneye executable. It
must report unsupported transport when ACK is configured only with a remote
MCP URL.

## Configuration and persistence

| Variable | Current behavior |
| --- | --- |
| `GOLDENEYE_DB_PATH` | Overrides the SQLite database path |
| `GOLDENEYE_PROJECT_ROOT` | Default repository root; otherwise current directory |
| `CBM_ALLOWED_ROOT` | Repository and mutation authorization boundary |
| `CBM_CACHE_DIR` | First database fallback directory |
| `CBM_SEMANTIC_ENABLED` | Parsed when the value starts with `1`, but currently unused |
| `CBM_SEMANTIC_THRESHOLD` | Parses `(0,1]`, default `0.75`, but currently unused |
| `CBM_WATCHER_PRUNE_GRACE_S` | Missing-root prune grace in seconds; default 600 |
| `CBM_PERSISTENCE` | HTTP indexing persistence default for `1`, `true`, `yes`, or `on` |
| `CBM_UI_LANG` | `en` or `fr`; otherwise inferred from `Accept-Language` |

Database fallback order is `CBM_CACHE_DIR`, `LOCALAPPDATA/codebase-memory-mcp`,
`XDG_CACHE_HOME/codebase-memory-mcp`, `HOME/.cache/codebase-memory-mcp`, then
`.goldeneye/goldeneye.db`.

Artifacts use schema version 2 and these repository files:

```text
.codebase-memory/graph.db.zst
.codebase-memory/artifact.json
```

Metadata records commit, indexed time, project, node/edge counts, original and
compressed sizes, compression level, and SHA-256. Both compressed and
decompressed payloads are limited to 64 MiB. `fast` uses zstd level 3 and
retains indexes; `best` strips explicit indexes and uses level 9. Export uses a
consistent `VACUUM INTO` snapshot and atomic file replacement. Import verifies
schema, paths, file types, size, checksum, decompression bounds, and SQLite
integrity before rollback-safe installation.

## Compatibility and migration notes

Resolve these before presenting every feature as ACK-ready:

1. **Block shared artifact import/export and `--persist`.** Export snapshots the
   entire configured multi-project database while metadata describes only one
   project. A repository artifact can therefore contain other projects. Import
   replaces the entire configured database and can erase unrelated local
   graphs. Services also persist with `best`, which strips 14 explicit SQLite
   indexes; import does not recreate them, so an imported artifact can suffer a
   major query and edit-journal performance regression. It is safe only with a
   dedicated database containing exactly one project, and `fast` is the only
   round-trip path currently covered by tests. The durable fix is project-scoped
   export plus merge/project-scoped import and deterministic index recreation.
2. **Do not claim `target_projects` filtering.** The MCP server echoes the
   argument, but `cross-repo-intelligence` currently rebuilds links for every
   indexed project.
3. **Do not claim full grammar runtime coverage.** The full pack declares 160
   languages and compiles 159, but `Services` constructs indexing and editing
   with `CoreGrammarProvider`. Shipped runtime grammar IDs are only `go`,
   `javascript`, `python`, `rust`, `tsx`, and `typescript`.
4. **Preflight custom names.** Project names are sanitized and can collide.
   `Store::register_project` updates the existing project's root when an ID
   already exists. ACK should compare `list_projects` before `--name` and fail
   on a different existing root unless replacement is explicit.
5. **Fix or special-case `delete_project` schema.** Its advertised schema is
   nested incorrectly even though flat runtime arguments work.
6. **Do not expose semantic environment switches as functional controls.**
   They are parsed into `ServiceConfig`, but semantic indexing/search does not
   consult them and no threshold is applied.
7. **Treat ADR sections and traces as partial.** `manage_adr.sections` is
   ignored and `sections` mode returns all headings. Trace ingestion persists
   bounded observations but does not yet create runtime graph edges.
8. **Artifact bootstrap is best-effort.** Import errors are discarded before
   normal indexing; ACK should not report a successful bootstrap unless it
   independently verifies the imported database.
9. **Expose watcher effects before watcher controls.** MCP and HTTP indexing
   start watchers automatically. Refresh uses `fast`, which clears semantic
   vectors; non-Git repositories are not refreshed; and a missing root is
   removed from the database after three misses and the prune grace (10 minutes
   by default). ACK needs watcher status and an explicit prune policy before it
   offers administration commands.
10. **Surface edit-recovery failures verbatim.** Unresolved durable-journal
    recovery conflicts block MCP startup, and no MCP/CLI recovery command exists
    yet. ACK must preserve the actionable startup error instead of reporting a
    generic transport failure.
11. **Keep UI on loopback.** `goldeneye ui --bind` can expose wildcard-CORS,
    unauthenticated project deletion and process-kill endpoints. ACK should
    reject non-loopback binds unless a future explicit unsafe-public option and
    authentication contract are added.
12. **Reload the server before ACK acceptance.** HEAD advertises
    `index_repository.name` and does not advertise
    `get_code_snippet.include_neighbors`. Seeing the reverse means ACK is
    connected to a stale development binary; rebuild/restart and compare
    `tools/list` with the frozen fixture.

Project-name overrides are otherwise propagated through MCP, HTTP indexing,
and watcher refresh. The resolved sanitized project ID is returned by indexing
and should become ACK's canonical project selector.

## Verification evidence

Fresh verification at commit `14cdb59`:

- `cargo fmt --check`, `cargo check`, and zero-warning Clippy passed.
- Rust: 73 test binaries/suites, 411 tests passed.
- UI: 13 test files, 44 tests passed.
- Production UI build passed without warnings.
- Isolated exact-HEAD self-index: 211 files, 15,487 nodes, 22,068 edges,
  0 diagnostics.
- ACK status and symbol search resolved `GoldeneyeBackend` from
  `crates/delivery/goldeneye-http/src/backend.rs`.

## Source map

- `crates/delivery/goldeneye-mcp/src/tools.rs`: canonical tool schemas.
- `crates/delivery/goldeneye-mcp/src/server.rs`: dispatch and compatibility behavior.
- `tests/fixtures/mcp/foundation.expected.jsonl`: frozen 21-tool protocol.
- `crates/application/goldeneye-services/src/edit.rs`: edit requests, results, and runtime.
- `crates/application/goldeneye-services/src/git.rs`: change-impact contract.
- `crates/application/goldeneye-services/src/adr_traces.rs`: ADR and trace contracts.
- `crates/application/goldeneye-index/src/enrichment.rs`: derived graph semantics.
- `crates/application/goldeneye-index/src/identity.rs`: project-name normalization.
- `crates/application/goldeneye-crosslink/src/lib.rs`: cross-project edge derivation.
- `crates/adapters/goldeneye-artifact/src/lib.rs`: artifact format and installation.
- `crates/adapters/goldeneye-store/src/lib.rs`: project registration and shared store.
- `crates/adapters/goldeneye-syntax/src/grammar.rs`: shipped core grammar provider.
- `crates/delivery/goldeneye-http/src/backend.rs`: HTTP API behavior.
- `crates/delivery/goldeneye-watcher/src/lib.rs`: watcher behavior and defaults.
- `crates/delivery/goldeneye-cli/src/main.rs`: direct CLI contract.
- `ui/HTTP_API_CONTRACT.md`: UI route contract.
- `docs/ack-acceptance.md`: current ACK Phase-1 baseline.
