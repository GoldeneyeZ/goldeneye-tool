# GCALSC-2 Spec Review

**Result:** checked
**Reviewed commit:** `3ed441e0eeec575f12c8019dfa22fb9f09979029`

## Evidence

- Reviewed complete changed ranges: `StdioCodebaseMemoryClient.ts` lines 1-191 and `stdioClient.test.ts` lines 1-174.
- The session lazily calls `spawn(command, [], { stdio: ["pipe", "pipe", "pipe"] })`, initializes once with protocol `2024-11-05`, and sends `notifications/initialized` before direct `tools/call` requests.
- `index_status` receives direct MCP arguments `{ project: "example-project" }`; gateway namespacing remains covered by existing gateway tests.
- Newline response handling ignores unrelated/malformed lines, correlates numeric IDs, rejects JSON-RPC errors and malformed matching responses, and rejects pending calls for child `error`, `exit`, and `close`.
- `close()` kills an active child and resolves only after `close`.
- Focused verification passed: `pnpm test tests/stdioClient.test.ts tests/gatewayClient.test.ts` — 20/20 tests.

No missing, extra, or misunderstood GCALSC-2 requirement found.
