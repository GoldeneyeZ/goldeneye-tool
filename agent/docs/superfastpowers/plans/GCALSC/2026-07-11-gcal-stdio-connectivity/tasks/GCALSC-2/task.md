### Task 2: Add direct stdio MCP transport

<TASK-ID>GCALSC-2</TASK-ID>

**Files:**
- Create: `src/adapters/codebaseMemoryMcp/StdioCodebaseMemoryClient.ts`
- Create: `tests/stdioClient.test.ts`

- [x] Use `PassThrough` streams and fake child-process events to write failing tests for MCP `initialize`, `notifications/initialized`, and direct `tools/call` for `index_status`.
- [x] Run `pnpm test tests/stdioClient.test.ts`; observe missing-client failure.
- [x] Implement a lazy, one-process JSON-RPC session using `spawn(command, [], { stdio: ["pipe", "pipe", "pipe"] })`. Parse newline-delimited stdout JSON objects, correlate numeric IDs, reject JSON-RPC errors, malformed responses, child errors, exits, and close before response.
- [x] Compose shared normalized client with the direct session invoker. Add `close(): Promise<void>` which terminates active child and waits for closure.
- [x] Run `pnpm test tests/stdioClient.test.ts tests/gatewayClient.test.ts`.
