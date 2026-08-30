# Agent Instructions

## Project

Goldeneye Code Agent Layer (GCAL) is a TypeScript CLI and workflow kit over Goldeneye. It provides deterministic context-election primitives that keep agent context compact and high-signal.

Phase 1 commands are:

- `gcal search`
- `gcal symbol`
- `gcal inspect`
- `gcal get`
- `gcal callers`
- `gcal callees`
- `gcal arch`
- `gcal status`
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

- `search` and `symbol`: compact tab-separated candidate rows.
- `inspect`: candidate block when applicable, selected metadata, warnings, inbound/outbound sections, no full source.
- `get`: source text only.
- `callers` and `callees`: headerless relationship rows only.
- `arch`, `status`, and `index`: compact JSON.

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
