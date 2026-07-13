# Rust-only ACK acceptance

Build and exercise the installed ACK CLI against only Goldeneye's MCP stdio binary:

```powershell
.\tools\ack-acceptance.ps1 -AckRoot C:\path\to\agent-context-kernel
```

Portable invocation:

```text
node tools/ack-acceptance.mjs --ack-root /path/to/agent-context-kernel
```

The harness copies the checked-in Rust fixture into a temporary project, uses a temporary SQLite database, removes `ACK_MCP_URL`, empties `PATH`, and sets `ACK_MCP_COMMAND` to Goldeneye's absolute binary path. It asserts the server identity, index result, all Phase 1 ACK command outputs, exact source, relationships, ambiguity, suggestions, project selection, compact output bounds, and absence of `ack elect`. Temporary state is deleted after the run.
