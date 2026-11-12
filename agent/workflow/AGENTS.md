# GCAL Workflow Rules

- While `gcal` is available, it is the sole model-facing code-discovery surface over Goldeneye.
- Use the installed `goldeneye-code-agent-layer` skill; one `gcal` call should answer one question.
- Name the target directly (`Type.member`, `File.ext:42`); never search only to learn an identifier.
- Batch: several names per `gcal get` or `gcal search`, several commands per shell call.
- Use direct Goldeneye MCP tools only when GCAL is unavailable or fails; never repeat the same query through both surfaces.
- Use `gcal` when the answer spans files: a flow, callers, implementations, several symbols at once, every use of a name, one edit in many files.
- Use plain file tools when one known file is enough: reading or editing a file whose path you already have.
- If GCAL reports no project for the current directory, run `gcal init` once from the project root.
