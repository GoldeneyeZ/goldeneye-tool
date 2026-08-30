# GCALSC-3 Spec Review

**Result:** checked
**Reviewed commit:** `d61ded922cb9760638c059f72c77c44e4712aa5c`

## Evidence

- Unset `GCAL_MCP_URL` passes `{ mcpUrl: undefined, mcpCommand: "codebase-memory-mcp", project: "" }`; explicit URL and command remain preserved by focused CLI tests.
- `main` selects gateway only when a URL exists, otherwise direct stdio. `runCli` awaits optional `close()` in `finally`.
- README accurately documents direct stdio default, explicit gateway compatibility, and command-path semantics.
- The direct-MCP `content[].text` wrapper originally caused `gcal arch` to print `{}`. Shared `unwrapMcpPayload` now provides gateway-equivalent unwrapping and tool `isError` handling; stdio architecture and error regressions protect this behavior.
- `pnpm check` passed: ESLint, 81 Vitest tests, and TypeScript build.
- Live no-URL verification emitted populated compact status and architecture JSON; both commands exited 0.

No missing, extra, or misunderstood GCALSC-3 requirement found.
