# Agent Instructions

## Project

Goldeneye Code Agent Layer (GCAL) is a TypeScript CLI and workflow kit over Goldeneye. It provides deterministic context-election primitives that keep agent context compact and high-signal.

Phase 1 commands are:

- `gcal search`
- `gcal symbol`
- `gcal inspect`
- `gcal get` (with `--tests`)
- `gcal outline`
- `gcal grep`
- `gcal replace`
- `gcal check`
- `gcal callers`
- `gcal callees`
- `gcal impls`
- `gcal workflow`
- `gcal arch`
- `gcal status`
- `gcal init`
- `gcal index`

Do not implement `gcal elect` yet. It is a later orchestration command.

## Development Rules

- Follow `dev-guidelines.md` for coding style, architecture, testing, and agent workflow.
- Keep Goldeneye as the default backend.
- Keep the compatibility MCP adapter isolated behind explicit benchmark mode.
- Prefer compact, context-safe outputs over raw MCP payloads.
- Use fixture-based tests for MCP behavior; do not require a live MCP server in normal tests.
- Never revert unrelated user changes.

## Tech Stack

- Runtime: Node.js 20+
- Language: TypeScript
- Package manager: pnpm
- CLI: Commander
- Validation: Zod
- Tests: Vitest
- Lint/format: ESLint and Prettier
- Build: `tsc`

## Common Commands

```bash
pnpm install
pnpm test
pnpm build
pnpm check
```

Use `pnpm check` before claiming implementation work is complete. For docs-only changes, no build is required unless the docs claim generated output or runtime behavior.

## Architecture Boundaries

- `src/cli/`: command parsing, command gating, stdout/stderr behavior.
- `src/adapters/goldeneye/`: Goldeneye default-backend wiring.
- `src/adapters/benchmark/`: explicit benchmark-backend wiring.
- `src/domain/`: GCAL-owned normalized types.
- `src/kernel/`: context-safety policy, trace thresholds, affordance warnings.
- `src/formatters/`: compact text/JSON output contracts.
- `src/workflows/` and `workflow/`: reusable agent workflow kit.
- `tests/`: fixture-based unit tests for output contracts and adapter behavior.

Do not let CLI code depend on raw MCP payload shapes. Normalize through the adapter first.

## Output Contracts

With the Goldeneye backend and a selected project, commands answer from the graph and the working tree:

- `search` and `symbol`: compact tab-separated candidate rows; `search` falls back to text when no symbol matches.
- `inspect`: candidate block when applicable, selected metadata, warnings, inbound/outbound sections, no full source.
- `get`: one `# name Kind path:start-end` header per item, then its source; an outline for a type or file above 500 lines; with `--tests`, the tests that use each symbol.
- `grep`, `callers`, `callees`, `impls`: the file path once, then indented rows that keep the line as it is in the file. A cut `grep` names the matching files it did not show.
- `callers` on a file: a header with the number of importing files and each spelling of the path, then per file the lines that name the module and a `uses` row with the lines using what it imports.
- A lookup that finds nothing prints `# not found`, `# ambiguous` or `# no text matches` and exits 0.
- `replace`: a count line, then each changed line by file, then where else the pattern still matches.
- `check`: the lines that do not parse in the files that differ from HEAD; then, when the index shows them, callers of a removed or renamed symbol that still name it, imports of a deleted file, and Java constructor calls with a number of arguments no constructor takes.
- `arch`: directory layout by file count. `status` and `index`: compact JSON.

The benchmark backend keeps the earlier contracts: `get` prints source text only, `callers` and `callees` print headerless relationship rows, `arch` prints compact JSON.

Do not add noisy headers, pretty JSON, raw payload dumps, or full source to commands whose contracts do not allow them.

## Context Election Rules

- Search cheaply before fetching source.
- Prefer exact qualified names when available.
- Use `inspect` before `get` when source inclusion is uncertain.
- Use `get` only when exact source earns context.
- Use caller/callee traces only when graph relationships answer the task.
- Preserve useful MCP labels in normalized output; do not collapse unknown labels unless no label exists.
- Keep `inspect` source-safe: selected source must not appear in inspect output.

## Configuration

GCAL defaults to:

```bash
GCAL_BACKEND=goldeneye
GCAL_GOLDENEYE_COMMAND=goldeneye
```

`GCAL_BACKEND=benchmark` explicitly selects the retained compatibility adapter. Only benchmark mode reads `GCAL_MCP_COMMAND` and `GCAL_MCP_URL`.

Project-backed commands resolve the current directory through `$HOME/.gcal/projects.json`. `GCAL_PROJECT` remains an optional explicit override.

`init` and `index` can run without `GCAL_PROJECT`; they only need a reachable selected backend.

Help commands must work without `GCAL_PROJECT`.

## Testing Guidance

- Add or update focused tests for every behavior change.
- For bug fixes, write a regression test that fails before the fix.
- Protect output contracts with exact string tests where practical.
- Keep adapter tests fixture-based and mock HTTP transport.
- Do not add live MCP server requirements to `pnpm test` or `pnpm check`.

## Documentation

- Keep `README.md` aligned with actual CLI behavior.
- Keep workflow-kit docs in `workflow/` productized for GCAL, not project-specific to another repo.
- If command behavior changes, update README and workflow instructions in the same change.
