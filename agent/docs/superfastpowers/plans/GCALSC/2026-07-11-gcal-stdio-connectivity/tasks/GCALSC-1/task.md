### Task 1: Extract shared direct-tool client

<TASK-ID>GCALSC-1</TASK-ID>

**Files:**
- Create: `src/adapters/codebaseMemoryMcp/CodebaseMemoryMcpClient.ts`
- Modify: `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`
- Modify: `tests/gatewayClient.test.ts`

- [x] Write a regression test proving `GatewayCodebaseMemoryClient.status()` sends `gateway.invoke` with `id: "codebase-memory-mcp::index_status"` and the configured project.
- [x] Run `pnpm test tests/gatewayClient.test.ts`; the pre-existing gateway contract passed, then a missing shared-client import produced the required red test run.
- [x] Add `McpToolInvoker` and `CodebaseMemoryMcpClient`. Move normalized search, symbol, get, trace, arch, status, and index operations from gateway client. Operations call direct names: `search_graph`, `get_code_snippet`, `trace_path`, `get_architecture`, `index_status`, and `index_repository`.
- [x] Refactor gateway client to compose shared client and prefix its requested tool name with `codebase-memory-mcp::` before using existing `gatewayInvoke`.
- [x] Run `pnpm test tests/gatewayClient.test.ts`; preserve all existing output/normalization behavior.
