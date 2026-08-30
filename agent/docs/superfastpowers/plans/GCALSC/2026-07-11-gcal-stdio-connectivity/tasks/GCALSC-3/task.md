### Task 3: Select transport, document configuration, verify live commands

<TASK-ID>GCALSC-3</TASK-ID>

**Files:**
- Modify: `src/cli/runCli.ts`
- Modify: `src/main.ts`
- Modify: `tests/cli.test.ts`
- Modify: `README.md`

- [x] Add failing CLI tests: absent `GCAL_MCP_URL` produces `{ mcpUrl: undefined, mcpCommand: "codebase-memory-mcp", project: "" }`; explicit URL is preserved; optional `close()` is awaited.
- [x] Run `pnpm test tests/cli.test.ts`; observe failure before implementation.
- [x] Add `mcpCommand` to client configuration. Choose gateway only when `mcpUrl` exists; otherwise choose stdio. Close disposable clients in `runCli` finally.
- [x] Document direct stdio default, explicit HTTP gateway compatibility, and `GCAL_MCP_COMMAND` executable-path semantics.
- [x] Run `pnpm check`.
- [x] Persist `GCAL_MCP_COMMAND=C:\Users\Zacha\AppData\Local\Programs\codebase-memory-mcp\codebase-memory-mcp.exe`, then run `gcal status` and `gcal arch` with the supplied project and no `GCAL_MCP_URL`; both exit 0 with populated compact JSON.
