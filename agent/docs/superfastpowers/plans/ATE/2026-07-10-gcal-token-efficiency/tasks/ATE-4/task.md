### Task 4: Install One GCAL Discovery Front Door

<TASK-ID>ATE-4</TASK-ID>

**Files:**
- Create: `tests/workflowFiles.test.ts`
- Modify: `tests/installScript.test.ts`
- Modify: `src/workflows/createWorkflowFiles.ts`
- Modify: `workflow/AGENTS.md`
- Modify: `workflow/skills/codebase-memory/SKILL.md`
- Modify: `workflow/skills/codebase-memory/agents/claude.md`
- Modify: `workflow/skills/codebase-memory/agents/openai.yaml`
- Modify: `install.ps1`

- [ ] **Step 1: Write the failing generated-asset parity and routing test**

```typescript
import { readFile } from "node:fs/promises";
import { describe, expect, it } from "vitest";
import { workflowFiles, workflowAgentsMd, codebaseMemorySkillMd } from "../src/workflows/createWorkflowFiles.js";

describe("workflow files", () => {
  it("keeps committed assets aligned with generated assets", async () => {
    for (const file of workflowFiles) {
      const committed = (await readFile(file.path, "utf8")).replace(/\r\n/g, "\n");
      expect(committed).toBe(file.content);
    }
  });

  it("routes each discovery need through one surface and one command path", () => {
    expect(workflowAgentsMd).toContain("sole model-facing code-discovery surface");
    expect(workflowAgentsMd).toContain("never repeat the same query through both surfaces");
    expect(codebaseMemorySkillMd).toContain("Exact qualified name and source needed");
    expect(codebaseMemorySkillMd).toContain("Do not run `search` before `inspect`");
    expect(codebaseMemorySkillMd).toContain("Stop discovery once the task has enough evidence");
  });
});
```

- [ ] **Step 2: Write the failing installer migration assertion**

Extend `tests/installScript.test.ts`:

```typescript
expect(script).toContain("<!-- codebase-memory-mcp:start -->");
expect(script).toContain("<!-- codebase-memory-mcp:end -->");
expect(script).toContain("Remove-ManagedBlock");
```

- [ ] **Step 3: Run the workflow and installer tests and verify they fail**

Run: `pnpm vitest run tests/workflowFiles.test.ts tests/installScript.test.ts`

Expected: FAIL because the workflow remains duplicated/serial and the installer does not migrate the legacy block.

- [ ] **Step 4: Replace the global workflow block with a short precedence rule**

Generate and commit this content:

```markdown
# GCAL Workflow Rules

- While `gcal` is available, it is the sole model-facing code-discovery surface and supersedes earlier direct codebase-memory-mcp discovery instructions.
- Use the installed `codebase-memory` skill to choose one GCAL command path.
- Use direct codebase-memory-mcp tools only when GCAL is unavailable or fails; never repeat the same query through both surfaces.
- Use raw text search for literals, configs, non-code files, or clearly weak GCAL results.
- Do not implement or rely on `gcal elect` in Phase 1.
```

- [ ] **Step 5: Replace the skill workflow with a mutually exclusive decision table**

The generated and committed skill must contain:

```markdown
## Choose One Route

| Need | Route |
| --- | --- |
| Exact qualified name and source needed | Run `gcal get` directly. |
| Unknown symbol and source needed | Run one `gcal search ... --limit 5` or `gcal symbol ... --limit 5`, select one result, then run `gcal get`. |
| Metadata determines whether source is needed | Run `gcal inspect` directly. Do not run `search` before `inspect`; broad inspect already searches. |
| Relationship or impact evidence | Run `gcal callers` or `gcal callees` with `--depth 1 --limit 20`. |
| High-level project shape | Run `gcal arch` once. |

Do not run `inspect` and then `get` when the task already requires source. Stop discovery once the task has enough evidence.
```

Keep the raw-text fallback and Phase 1 `elect` boundary, but remove duplicate prose already present in the global block.

Update the two agent integration files to use these compact prompts:

```markdown
# Codebase Memory

Use the `codebase-memory` skill to choose exactly one GCAL discovery route. While GCAL is available, do not repeat discovery through direct codebase-memory-mcp tools.
```

```yaml
interface:
  display_name: "Codebase Memory"
  short_description: "Choose one compact GCAL discovery route"
  default_prompt: "Use $codebase-memory to choose one GCAL route. Do not repeat the query through direct MCP tools."
```

- [ ] **Step 6: Add legacy managed-block removal to the installer**

```powershell
function Remove-ManagedBlock {
  param(
    [Parameter(Mandatory = $true)][string]$Path,
    [Parameter(Mandatory = $true)][string]$StartMarker,
    [Parameter(Mandatory = $true)][string]$EndMarker
  )

  if (-not (Test-Path -LiteralPath $Path)) { return }

  $existing = Get-Content -LiteralPath $Path -Raw
  $pattern = "(?s)$([regex]::Escape($StartMarker)).*?$([regex]::Escape($EndMarker))\s*"
  $updated = [regex]::Replace($existing, $pattern, "").TrimEnd()
  if ($updated.Length -gt 0) { $updated += [Environment]::NewLine }
  Set-Content -LiteralPath $Path -Value $updated -Encoding utf8
}
```

Immediately before `Set-ManagedBlock`, call:

```powershell
Remove-ManagedBlock `
  -Path $codexAgents `
  -StartMarker "<!-- codebase-memory-mcp:start -->" `
  -EndMarker "<!-- codebase-memory-mcp:end -->"
```

- [ ] **Step 7: Run the workflow and installer tests and verify they pass**

Run: `pnpm vitest run tests/workflowFiles.test.ts tests/installScript.test.ts`

Expected: PASS.

- [ ] **Step 8: Commit the single-front-door workflow**

```bash
git add tests/workflowFiles.test.ts tests/installScript.test.ts src/workflows/createWorkflowFiles.ts workflow install.ps1
git commit -m "perf: route discovery through GCAL only"
```
