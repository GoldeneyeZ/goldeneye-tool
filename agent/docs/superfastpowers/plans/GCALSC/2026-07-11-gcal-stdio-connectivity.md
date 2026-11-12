# GCAL Stdio Connectivity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:subagent-driven-development (recommended), superfastpowers:goal-driven-development, or superfastpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make GCAL invoke the installed `codebase-memory-mcp` stdio server when no HTTP gateway URL is configured.

**Architecture:** Extract GCAL's normalized codebase-memory operations behind an MCP-tool invoker. Retain the current HTTP gateway adapter for explicit `GCAL_MCP_URL`; add a stdio JSON-RPC session adapter for direct tool calls. Main selects transport from configuration and closes stdio processes after each CLI invocation.
**Plan Acronym:** GCALSC


**Tech Stack:** Node.js 20, TypeScript, Commander, Vitest, MCP JSON-RPC over stdio

---

## File Structure

- `src/adapters/codebaseMemoryMcp/CodebaseMemoryMcpClient.ts`: shared normalized GCAL operations using an injected MCP tool invoker.
- `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`: HTTP gateway implementation of the invoker.
- `src/adapters/codebaseMemoryMcp/StdioCodebaseMemoryClient.ts`: direct stdio MCP client and process lifecycle.
- `src/cli/runCli.ts`: transport configuration and optional client shutdown.
- `src/main.ts`: chooses gateway vs. stdio client.
- `tests/gatewayClient.test.ts`: protects existing gateway request contract.
- `tests/stdioClient.test.ts`: covers initialization, direct calls, and failures using stream fixtures.
- `tests/cli.test.ts`: covers transport configuration and close lifecycle.
- `README.md`: documents `GCAL_MCP_URL` and `GCAL_MCP_COMMAND` transport selection.

### Task 1: Extract shared direct-tool client

<TASK-ID>GCALSC-1</TASK-ID>

**Files:**
- Create: `src/adapters/codebaseMemoryMcp/CodebaseMemoryMcpClient.ts`
- Modify: `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`
- Modify: `tests/gatewayClient.test.ts`

- [x] **Step 1: Write failing shared-client gateway regression test**

Add a test that calls `GatewayCodebaseMemoryClient.status()` and asserts the HTTP request still uses `gateway.invoke` with `id: "codebase-memory-mcp::index_status"` and `{ project: "example-project" }`.

- [x] **Step 2: Run test to verify it fails after a temporary missing shared client import**

Run: `pnpm test tests/gatewayClient.test.ts`
Expected: FAIL because `CodebaseMemoryMcpClient` is not exported.

- [x] **Step 3: Add shared normalized operations**

Create an `McpToolInvoker` contract and `CodebaseMemoryMcpClient` implementation:

```ts
export interface McpToolInvoker {
  invoke(toolName: string, args: Record<string, unknown>): Promise<unknown>;
}

export class CodebaseMemoryMcpClient implements CodebaseMemoryClient {
  constructor(private readonly project: string, private readonly invoker: McpToolInvoker) {}
  // Move search, symbol, get, callers, callees, arch, status, index and trace helpers here.
}
```

Use direct names such as `index_status`, `search_graph`, and `get_architecture`; retain all existing normalizers and trace mapping behavior.

- [x] **Step 4: Make gateway delegate to shared client**

```ts
private readonly client = new CodebaseMemoryMcpClient(this.config.project, {
  invoke: (toolName, args) => gatewayInvoke({
    mcpUrl: this.config.mcpUrl,
    toolId: `codebase-memory-mcp::${toolName}`,
    args,
    fetch: this.fetchImpl,
  }),
});
```

Delegate all public methods to `this.client`.

- [x] **Step 5: Run focused tests**

Run: `pnpm test tests/gatewayClient.test.ts`
Expected: PASS; all gateway request IDs remain namespaced.

### Task 2: Add direct stdio MCP transport

<TASK-ID>GCALSC-2</TASK-ID>

**Files:**
- Create: `src/adapters/codebaseMemoryMcp/StdioCodebaseMemoryClient.ts`
- Create: `tests/stdioClient.test.ts`

- [x] **Step 1: Write failing stdio transport tests**

Build `PassThrough` stream fixtures and a fake child process. Assert that `status()` writes, in order:

```ts
{ jsonrpc: "2.0", id: 1, method: "initialize" }
{ jsonrpc: "2.0", method: "notifications/initialized" }
{ jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: "index_status", arguments: { project: "example-project" } } }
```

Also assert response correlation, JSON-RPC error text, malformed lines ignored, and process-close rejection.

- [x] **Step 2: Run tests to verify they fail**

Run: `pnpm test tests/stdioClient.test.ts`
Expected: FAIL because `StdioCodebaseMemoryClient` is missing.

- [x] **Step 3: Implement one-process JSON-RPC stdio session**

Use `node:child_process` `spawn(command, [], { stdio: ["pipe", "pipe", "pipe"] })`. Split stdout by newline, parse only JSON object lines, correlate numeric request IDs, and initialize lazily once. Send direct MCP `tools/call` requests. Reject all pending requests on child error, exit, or close. Do not shell-parse `GCAL_MCP_COMMAND`.

- [x] **Step 4: Implement stdio normalized client and shutdown**

Compose `CodebaseMemoryMcpClient` with the session invoker. Expose `close(): Promise<void>` that terminates an active child process and resolves when it closes.

- [x] **Step 5: Run focused tests**

Run: `pnpm test tests/stdioClient.test.ts tests/gatewayClient.test.ts`
Expected: PASS.

### Task 3: Select transport, document configuration, verify live commands

<TASK-ID>GCALSC-3</TASK-ID>

**Files:**
- Modify: `src/cli/runCli.ts`
- Modify: `src/main.ts`
- Modify: `tests/cli.test.ts`
- Modify: `README.md`

- [x] **Step 1: Write failing CLI configuration tests**

Assert `runCli` passes `{ mcpUrl: undefined, mcpCommand: "codebase-memory-mcp", project: "" }` when `GCAL_MCP_URL` is absent, and preserves an explicit URL while passing `GCAL_MCP_COMMAND` when both are set. Add an optional fake `close` method and assert it is awaited after command execution.

- [x] **Step 2: Run test to verify it fails**

Run: `pnpm test tests/cli.test.ts`
Expected: FAIL because `ClientConfig` lacks `mcpCommand` and no shutdown occurs.

- [x] **Step 3: Select transport and close disposable clients**

Set `mcpUrl` to `options.env.GCAL_MCP_URL`, set `mcpCommand` to `options.env.GCAL_MCP_COMMAND ?? "codebase-memory-mcp"`, and close clients implementing `close(): Promise<void>` in `runCli`'s `finally`. In `main.ts`, instantiate `GatewayCodebaseMemoryClient` only when `mcpUrl` exists; otherwise instantiate `StdioCodebaseMemoryClient`.

- [x] **Step 4: Update README configuration**

Document HTTP gateway compatibility as opt-in via `GCAL_MCP_URL`; document direct stdio default and `GCAL_MCP_COMMAND` as an executable path, not a shell command.

- [x] **Step 5: Run complete verification**

Run: `pnpm check`
Expected: exit 0.

- [x] **Step 6: Configure and test real server**

Set user environment variable:

```powershell
[Environment]::SetEnvironmentVariable(
  "GCAL_MCP_COMMAND",
  "C:\Users\Zacha\AppData\Local\Programs\codebase-memory-mcp\codebase-memory-mcp.exe",
  "User"
)
```

Then run:

```powershell
$env:GCAL_MCP_COMMAND = "C:\Users\Zacha\AppData\Local\Programs\codebase-memory-mcp\codebase-memory-mcp.exe"
$env:GCAL_PROJECT = "C-Users-Zacha-WebstormProjects-revi-front-microservice"
Remove-Item Env:GCAL_MCP_URL -ErrorAction SilentlyContinue
gcal status
gcal arch
```

Expected: both exit 0 and emit compact JSON for the indexed project.
