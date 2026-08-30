# GCALSC-3 Implementer Handoff

Implementation commit `d61ded922cb9760638c059f72c77c44e4712aa5c` selects stdio when `GCAL_MCP_URL` is absent and keeps HTTP gateway selection explicit.

- `runCli` forwards optional `mcpUrl`, default `mcpCommand`, and project; its `finally` awaits disposable client shutdown.
- `main` chooses `StdioCodebaseMemoryClient` only without a URL.
- Direct stdio reuses gateway MCP payload unwrapping, including content JSON and `isError` behavior; this fixes `gcal arch` normalization.
- README documents default stdio, opt-in gateway URL, and executable-path command semantics.
- User `GCAL_MCP_COMMAND` now persists at required executable path. No active findings remain.
