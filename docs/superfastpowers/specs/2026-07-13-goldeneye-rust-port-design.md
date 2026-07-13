# Goldeneye Rust Port and Agent-Native Editing Design

Date: 2026-07-13

## Decision

Goldeneye will become a Rust rewrite of `DeusData/codebase-memory-mcp`, preserving its externally useful MCP and index compatibility while replacing internal C implementation with bounded Rust crates. Delivery uses vertical compatibility slices. Structural source editing and file creation land after the fast index and ACK-critical query slice; remaining upstream features then continue until full required parity.

Reference source:

- Repository: `https://github.com/DeusData/codebase-memory-mcp.git`
- Audited commit: `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`
- Local read-only checkout: `.upstream/codebase-memory-mcp`
- License: MIT. Goldeneye must retain upstream copyright and permission notices. Vendored grammar, model, generated-data, and UI notices remain separately attributable.

## Product Goal

Goldeneye is a headless CLI and MCP server for agent-native software development. Its primary advantage is token-efficient, context-efficient code navigation and mutation. It must:

1. Replace upstream C/C++ runtime with Rust.
2. Preserve enough upstream MCP behavior and graph semantics for ACK to operate against Goldeneye.
3. Support all Tree-sitter named nodes through generic structural edit locators.
4. Create source files with parse validation.
5. Reject stale or ambiguous edits instead of guessing.
6. Refresh graph state immediately after mutations.
7. Eventually port remaining upstream features required for functional parity, rather than treating the first editable slice as completion.

Goldeneye complements Context Mode. It does not duplicate shell, test, log, bulk-output, or web-output compression.

## Selected Delivery Strategy

Vertical compatibility slices:

1. Freeze observable upstream contracts and fixtures.
2. Establish Rust workspace, domain model, MCP transport, configuration, and compatibility harness.
3. Port discovery, Tree-sitter integration, graph storage, fast indexing, and stable identities.
4. Port ACK-critical read/query tools.
5. Add structural edit and create tools with targeted refresh.
6. Port remaining enrichment, analysis, runtime, artifact, UI, and packaging features.
7. Prove full objective through black-box parity, ACK acceptance, mutation, recovery, and cross-platform tests.

Only implemented MCP tools are advertised during migration. This prevents clients from treating stubbed behavior as working parity.

## Architecture

```text
goldeneye-cli
    |
goldeneye-mcp
    |
goldeneye-services
    |-- goldeneye-query
    |-- goldeneye-edit
    `-- goldeneye-index
           |
    +------+--------+
    |               |
goldeneye-syntax  goldeneye-store
    |               |
Tree-sitter       SQLite/FTS5
           \       /
         goldeneye-domain
```

### Crates

`goldeneye-domain`

- Owns project IDs, file IDs, graph nodes/edges, source spans, syntax locators, hashes, generations, tool-neutral requests/results, and typed errors.
- Has no MCP, SQLite, filesystem, or Tree-sitter dependency.

`goldeneye-syntax`

- Owns language detection, grammar registry, parser reuse, syntax-tree inspection, generic named-node locators, source extraction, and post-edit parse validation.
- Initially compiles upstream vendored C parsers and scanners through Rust build scripts. Grammar replacement is not required for initial Rust runtime completion.
- Supports malformed pre-edit files for inspection, but successful structural mutations must satisfy configured parse-error policy.

`goldeneye-store`

- Owns SQLite/FTS5 schema, migrations, transactions, graph persistence, project registry, file hashes, generations, query primitives, and artifact compatibility.
- Keeps MCP formatting outside storage.
- Opens query-only connections with restrictive SQLite authorization where practical.

`goldeneye-index`

- Owns repository discovery, ignore rules, language classification, FQN generation, parallel extraction, graph assembly, enrichment passes, incremental reconciliation, targeted file refresh, and watcher integration.
- Exposes service traits; callers do not manipulate graph buffers or SQLite directly.

`goldeneye-query`

- Implements graph search, Cypher execution, call/data-flow tracing, source snippets, architecture summaries, schema discovery, enriched code search, change detection, and semantic/similarity queries.
- Separates query-domain results from MCP JSON/TOON rendering.

`goldeneye-edit`

- Implements structural inspection, source creation, named-node replacement/deletion/insertion, journaling, stale-locator checks, parse validation, atomic replacement, rollback, and targeted reindex.
- Never accepts an unchecked byte range as authoritative.

`goldeneye-services`

- Orchestrates project resolution, authorization, indexing, queries, edits, cancellation, progress, watcher registration, and recovery.
- Defines the only entry point used by CLI and MCP layers.

`goldeneye-mcp`

- Implements JSON-RPC 2.0 and MCP lifecycle, stdio framing, cancellation, schema advertisement, pagination, JSON/TOON rendering, structured content, and tool-to-service mapping.
- Never accesses SQLite, Tree-sitter, or project files directly.

`goldeneye-cli`

- Owns executable startup, configuration commands, MCP stdio mode, watcher mode, optional HTTP/UI mode, installation helpers, and version output.

`goldeneye-compat-tests`

- Runs black-box requests against upstream and Rust binaries, normalizes intentional nondeterminism, and compares contracts, fixtures, graph facts, query results, and failure behavior.

## Compatibility Boundary

### ACK-Critical Slice

Before structural editing is considered usable, Rust must provide:

- MCP initialization, ping, `tools/list`, `tools/call`, cancellation, stdio framing, and response envelopes.
- `index_repository` in fast mode.
- `list_projects`.
- `index_status`.
- `get_graph_schema`.
- `search_graph` with pagination and compact output.
- `query_graph` for ACK-used Cypher patterns.
- `trace_path` and compatibility alias `trace_call_path`.
- `get_code_snippet` with exact, suffix, unique-short-name, ambiguity, and suggestion behavior.
- `get_architecture`.
- Stable project naming, file locations, qualified names, source spans, and graph refresh.

ACK acceptance must exercise `ack status`, `ack search`, `ack symbol`, `ack inspect`, `ack get`, `ack callers`, `ack callees`, and `ack arch` against only the Rust server.

### Eventual Upstream Parity

Later slices port, rather than discard:

- Moderate and full index modes.
- 159-language extraction behavior.
- Hybrid-LSP resolution.
- Semantic/vector search and bundled model data.
- Similarity and SimHash edges.
- Full custom Cypher behavior.
- Enriched `search_code`.
- Git history, co-change, and `detect_changes` blast radius.
- Cross-repository HTTP, async, and channel linking.
- Runtime trace ingestion.
- ADR management.
- Background watcher and automatic indexing.
- Compressed shared graph artifacts.
- Graph UI and HTTP server.
- npm, PyPI, Go, Homebrew, Chocolatey, Nix, and platform release packaging.
- Remaining upstream MCP formatting, pagination, error, configuration, and edge-case compatibility.

## Structural Editing Contract

### Locator

Every named Tree-sitter node may be addressed by a locator containing:

- canonical project ID;
- normalized project-relative file path;
- language and grammar ABI;
- Tree-sitter node kind;
- named-child ancestor path;
- original byte range;
- original node-content hash;
- original file-content hash;
- index/file generation.

Byte range accelerates lookup but never establishes identity alone. A mutation proceeds only when every required identity guard still matches. Any drift returns a stale-locator error with a fresh compact syntax view; Goldeneye does not fuzzy-relocate in the first editing contract.

### MCP Tools

`inspect_syntax`

- Returns compact named-node structure and locators for a file or bounded source region.
- Supports depth, node-kind, and result limits.

`create_file`

- Creates one new project-relative file.
- Rejects existing destination unless an explicit future overwrite operation is designed.
- Validates path containment and optionally requires parse validity when language is supported.

`replace_node`

- Replaces exactly one located named node.

`delete_node`

- Deletes exactly one located named node.

`insert_before_node` / `insert_after_node`

- Inserts content adjacent to exactly one located named node.

All mutation results contain minimal diff, changed file hash, changed symbol/node identities, diagnostics, index generation, and token-oriented size metadata.

## Mutation Data Flow

1. MCP parses request and invokes `goldeneye-services`.
2. Service resolves canonical project root and checks allowed-root policy.
3. Edit service loads current file bytes and generation.
4. Syntax service verifies file hash, node hash, node kind, ancestor path, and range.
5. Edit is applied in memory.
6. Syntax service parses proposed bytes and enforces parse-error policy.
7. Edit journal records operation ID, original/new hashes, target path, temporary path, backup path, and phase.
8. New bytes are written to a same-filesystem temporary file, flushed, then atomically renamed over destination.
9. Index service reparses changed file and commits graph/file-hash/generation updates in one SQLite transaction.
10. Journal is marked committed and temporary recovery material is removed.
11. Service returns minimal diff and refreshed identities.

Filesystem and SQLite cannot share one native transaction. Journaled recovery provides crash consistency. If targeted reindex fails after file replacement, Goldeneye restores original bytes when safe and reindexes original state. On startup, incomplete journals are reconciled against actual file hashes; actual source files remain authoritative.

## File Creation Data Flow

1. Resolve and authorize project-relative destination.
2. Reject absolute paths, traversal, symlink escapes, reserved metadata locations, and existing targets.
3. Detect language from filename or explicit supported language.
4. Parse candidate content when grammar exists.
5. Create missing parent directories only within project root when requested.
6. Write through journal and atomic rename.
7. Target-index new file in one SQLite transaction.
8. Return file identity, syntax root locator, graph changes, diagnostics, and generation.

## Errors and Safety

Typed failures include:

- project not found;
- path outside allowed root;
- unsupported language;
- syntax parse failure;
- stale file, node, or index generation;
- ambiguous node;
- destination exists;
- invalid insertion position;
- write, flush, rename, rollback, or database failure;
- cancelled request;
- recovery required.

Safety rules:

- Canonicalize project roots and validate normalized relative paths.
- Revalidate containment around filesystem mutation to reduce symlink races.
- Never follow a destination symlink outside project root.
- Enforce `CBM_ALLOWED_ROOT`-compatible policy.
- Reserve stdout for MCP JSON; diagnostics use stderr.
- Set request, result, file-size, syntax-depth, node-count, query-row, and memory limits.
- Never silently overwrite existing files.
- Never apply stale or fuzzy structural anchors.
- Never report graph refresh success before SQLite commit.
- Preserve original content and recovery data until operation completion is durable.

## Testing Strategy

### Contract Tests

- Port upstream MCP tests for initialization, protocol versions, request IDs, invalid JSON, unknown methods/tools, discovery probes, pagination, JSON/TOON envelopes, structured content, cancellation, and stdout purity.
- Compare advertised tool schemas against frozen snapshots.

### Index and Query Parity

- Run upstream and Rust binaries against identical fixture repositories.
- Compare normalized project IDs, file coverage, node labels, qualified names, source spans, edge types, graph schema, snippets, search ordering constraints, traces, architecture facts, and Cypher results.
- Maintain fixtures for spaces, Unicode/CJK paths, invalid UTF-8, symlinks, ignored files, deleted files, renamed files, and repository roots expressed through `.` or `..`.

### Syntax and Edit Tests

- Grammar fixtures verify every supported named-node kind can be inspected and addressed generically.
- Unit tests cover replace/delete/insert/create, stale hashes, stale generations, parse rejection, newline/encoding preservation, BOM handling, empty files, comments, and scanner-backed grammars.
- Property tests generate source edits and assert locator uniqueness, unchanged-region preservation, minimal diffs, and post-edit parser invariants.

### Durability Tests

- Fault injection at journal write, temp write, flush, rename, graph transaction, rollback, and cleanup phases.
- Kill/restart tests prove recovery converges database facts to actual source bytes.
- Concurrent-reader, concurrent-index, and competing-edit tests prove locking and stale rejection.

### ACK Acceptance

- Configure ACK to launch only Goldeneye Rust MCP.
- Execute all Phase-1 ACK commands on small fixtures and representative real repositories.
- Assert no fallback to upstream binary, expected compact fields, exact source retrieval, caller/callee paths, architecture output, and index freshness after Goldeneye mutation.

### Cross-Platform and Release Tests

- Linux, macOS, and Windows; x64 and arm64 where CI runners permit.
- Static or self-contained release artifact smoke tests.
- License ledger and generated notices checked in CI.
- Packaging shims validated against checksums and version metadata.

## Delivery Phases and Gates

### Phase 0: Contract Freeze

- Capture upstream schemas, response fixtures, SQLite schema, graph artifacts, FQN behavior, and core test corpora at audited commit.
- Gate: fixtures reproducible from read-only upstream checkout.

### Phase 1: Rust Foundation

- Workspace, domain types, config, logging, MCP transport, cancellation, service traits, test harness, license ledger.
- Gate: protocol/transport tests pass; implemented tool list is truthful.

### Phase 2: Fast Index Core

- Discovery, language registry, vendored grammar build, syntax extraction, graph model, SQLite/FTS5 store, project IDs, FQNs, fast index.
- Gate: normalized core graph fixtures match upstream on selected languages and path cases.

### Phase 3: ACK-Critical Query Slice

- Required read tools, compact output, targeted incremental refresh.
- Gate: complete ACK acceptance suite passes against Rust only.

### Phase 4: Structural Edit and Create

- Syntax inspection, locators, mutation journal, edit tools, create tool, diff result, targeted refresh, recovery.
- Gate: edit/create, stale/conflict, parse, durability, and ACK-post-edit freshness suites pass.

### Phase 5: Enrichment and Analysis Parity

- Moderate/full extraction, hybrid LSP, semantic/similarity, complete Cypher, search code, change analysis, cross-repo links, traces, ADR.
- Gate: normalized upstream-vs-Rust feature fixtures pass.

### Phase 6: Runtime and Delivery Parity

- Watcher, shared artifacts, UI/HTTP, packaging, all platform releases.
- Gate: end-to-end install/index/query/edit/recover/uninstall tests pass on supported platforms.

### Phase 7: Completion Audit

- Audit every upstream advertised capability, Goldeneye editing requirement, ACK command, platform, configuration contract, and license obligation against authoritative test/runtime evidence.
- Goal is complete only when no required item is missing, weakly evidenced, or dependent on upstream C runtime.

## Completion Criteria

Goldeneye is complete when:

1. Production runtime contains Rust implementation and allowed vendored grammar/parser assets, with no call into upstream C application code.
2. Required upstream behavior and eventual parity scope pass black-box compatibility gates.
3. ACK operates through Goldeneye for discovery, inspection, source retrieval, call tracing, and architecture without upstream fallback.
4. Generic named-node edits and file creation work across supported grammars, reject stale state, preserve unrelated bytes, validate syntax, recover from interruption, and refresh graph facts.
5. Cross-platform release, packaging, security, durability, and license tests pass.
6. Completion audit maps every stated requirement to current authoritative evidence.

