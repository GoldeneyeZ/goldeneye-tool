# GCAL Stdio Connectivity Design

## Problem

`codebase-memory-mcp.exe` is configured in Codex as an MCP stdio server. It does not listen at `http://localhost:8767/mcp`. GCAL currently defaults to that URL and invokes a non-standard `gateway.invoke` tool, so `gcal status` fails before reaching the indexed project.

## Goal

Make GCAL work directly with the installed stdio `codebase-memory-mcp` server while retaining explicitly configured gateway HTTP compatibility.

## Configuration

- If `GCAL_MCP_URL` is set, GCAL continues using the existing HTTP gateway transport.
- If `GCAL_MCP_URL` is unset, GCAL uses stdio transport.
- `GCAL_MCP_COMMAND` selects the stdio executable and defaults to `codebase-memory-mcp`.
- The current machine will set `GCAL_MCP_COMMAND` to `C:\Users\Zacha\AppData\Local\Programs\codebase-memory-mcp\codebase-memory-mcp.exe` so GCAL does not depend on `PATH`.

## Architecture

Introduce a small transport boundary beneath `GatewayCodebaseMemoryClient`'s existing normalized command methods.

- HTTP transport keeps current JSON-RPC request and `gateway.invoke` wrapping.
- Stdio transport spawns one MCP child process per GCAL invocation, sends `initialize`, sends `notifications/initialized`, then calls direct `codebase-memory-mcp` tool names via `tools/call`.
- The child process is reused for all command operations in that CLI run, including multi-call `inspect`.
- Both transports return the same raw tool result shape to existing normalizers and formatters.

## Error Handling

- Surface JSON-RPC errors as concise MCP errors.
- Reject pending calls when stdio closes or exits before a response.
- Include process-launch failures and malformed JSON-RPC responses without raw dumps.
- Use direct tool names only for stdio; gateway namespacing stays HTTP-only.

## Testing

- Unit-test stdio initialize, notification, direct `tools/call`, response correlation, and process-error handling with fixture streams.
- Regression-test runtime transport selection: no `GCAL_MCP_URL` selects stdio; explicit URL still selects gateway.
- Preserve existing HTTP gateway tests.
- Run `pnpm check`.
- Configure the user-level command path and verify live `gcal status` and `gcal arch` against `C-Users-Zacha-WebstormProjects-revi-front-microservice`.

## Non-Goals

- No HTTP listener or proxy process.
- No changes to command output contracts.
- No `gcal elect` implementation.
