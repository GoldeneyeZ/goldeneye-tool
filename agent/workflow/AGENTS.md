# GCAL Workflow Rules

- While `gcal` is available, it is the sole model-facing code-discovery surface over Goldeneye.
- Goldeneye is GCAL's default backend. Use `GCAL_BACKEND=benchmark` only for explicit compatibility measurements.
- Use the installed `goldeneye-code-agent-layer` skill to choose one GCAL command path.
- Use direct Goldeneye MCP tools only when GCAL is unavailable or fails; never repeat the same query through both surfaces.
- Use raw text search for literals, configs, non-code files, or clearly weak GCAL results.
- If GCAL reports no project for the current directory, run `gcal init` once from the project root.
- Do not implement or rely on `gcal elect` in Phase 1.
