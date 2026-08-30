# Rust-only GCAL acceptance

Build and exercise the installed GCAL CLI against only Goldeneye's MCP stdio binary:

```powershell
.\tools\gcal-acceptance.ps1
```

Portable invocation:

```text
node tools/gcal-acceptance.mjs
```

Both commands use `agent/` by default. Pass `-GcalRoot` or `--gcal-root` only to test another build.

The harness copies the checked-in Rust fixture into a temporary project, uses a temporary SQLite database, disables daemon reuse, removes benchmark-backend variables, empties `PATH`, and sets `GCAL_GOLDENEYE_COMMAND` to Goldeneye's absolute binary path. It asserts the server identity, index result, all Phase 1 GCAL command outputs, exact source, relationships, ambiguity, suggestions, project selection, compact output bounds, and absence of `gcal elect`. Temporary state is deleted after the run.
