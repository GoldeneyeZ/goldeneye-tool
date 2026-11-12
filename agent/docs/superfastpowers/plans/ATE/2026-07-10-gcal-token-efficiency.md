# GCAL Token Efficiency Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:subagent-driven-development (recommended), superfastpowers:goal-driven-development, or superfastpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bound GCAL's agent-facing context, restore live relationship results, and route agents through one non-duplicative GCAL discovery workflow.

**Architecture:** Normalize current and legacy MCP payloads inside the adapter, then enforce compact output limits at the CLI/formatter boundary. Keep the global GCAL block short and put mutually exclusive command routing in the installed skill; the installer removes the obsolete direct-MCP instruction block without disabling the fallback server.
**Plan Acronym:** ATE


**Tech Stack:** Node.js 20+, TypeScript, Commander, Zod, Vitest, PowerShell

---

## File Map

- `src/domain/types.ts`: add trace hop metadata.
- `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`: normalize live trace arrays and request/project bounded architecture.
- `src/adapters/codebaseMemoryMcp/normalize.ts`: project architecture responses onto the GCAL-owned compact contract.
- `src/adapters/codebaseMemoryMcp/mcpSchemas.ts`: validate architecture records.
- `src/formatters/textFormatters.ts`: emit non-duplicative relationship rows.
- `src/cli/createProgram.ts`: lower defaults and cap standalone trace rows.
- `src/workflows/createWorkflowFiles.ts`: generate the single-front-door global block and decision-table skill.
- `workflow/AGENTS.md`: committed short global routing override.
- `workflow/skills/codebase-memory/SKILL.md`: committed detailed mutually exclusive routing.
- `install.ps1`: remove the obsolete direct-MCP managed instruction block before installing GCAL rules.
- `README.md`: document output bounds, routing, and installer migration.
- `tests/fixtures/codebaseMemory.ts`: current and legacy MCP fixtures plus an oversized architecture fixture.
- `tests/gatewayClient.test.ts`: adapter contract tests.
- `tests/formatters.test.ts`: compact trace-row contract tests.
- `tests/cli.test.ts`: default, limit, and truncation tests.
- `tests/workflowFiles.test.ts`: generated/committed workflow parity and routing tests.
- `tests/installScript.test.ts`: legacy instruction migration and README behavior tests.

### Task 1: Restore Live Trace Results and Compact Rows

<TASK-ID>ATE-1</TASK-ID>

**Files:**
- Modify: `tests/fixtures/codebaseMemory.ts`
- Modify: `tests/gatewayClient.test.ts`
- Modify: `tests/formatters.test.ts`
- Modify: `src/domain/types.ts`
- Modify: `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`
- Modify: `src/formatters/textFormatters.ts`

- [ ] **Step 1: Replace the primary trace fixture with the live MCP shape and retain a legacy fixture**

```typescript
export const inboundTraceResponse = {
  function: "com.example.booking.BookingService.cancelBooking",
  direction: "inbound",
  mode: "calls",
  callers: [
    {
      name: "cancelBooking",
      qualified_name: "com.example.booking.BookingController.cancelBooking",
      hop: 1,
      file_path: "src/main/java/com/example/booking/BookingController.java",
      start_line: 31,
    },
  ],
};

export const outboundTraceResponse = {
  function: "com.example.booking.BookingService.cancelBooking",
  direction: "outbound",
  mode: "calls",
  callees: [
    {
      name: "findActiveBooking",
      qualified_name: "com.example.booking.BookingRepository.findActiveBooking",
      hop: 1,
      file_path: "src/main/java/com/example/booking/BookingRepository.java",
      start_line: 73,
    },
  ],
};

export const legacyInboundTraceResponse = {
  paths: [
    {
      caller: "com.example.booking.BookingController.cancelBooking",
      callee: "com.example.booking.BookingService.cancelBooking",
      file_path: "src/main/java/com/example/booking/BookingController.java",
      start_line: 31,
    },
  ],
};
```

- [ ] **Step 2: Write failing adapter tests for current inbound, current outbound, and legacy payloads**

Add assertions that `client.callers(...)` and `client.callees(...)` return explicit endpoints, the related qualified name, location, and `hop: 1`. Add a legacy assertion with `hop: null`.

```typescript
expect(await client.callers(selectedName, { depth: 1 })).toEqual([
  {
    sourceQualifiedName: "com.example.booking.BookingController.cancelBooking",
    targetQualifiedName: selectedName,
    relatedQualifiedName: "com.example.booking.BookingController.cancelBooking",
    hop: 1,
    filePath: "src/main/java/com/example/booking/BookingController.java",
    line: 31,
  },
]);
```

- [ ] **Step 3: Write the failing formatter test for the new four-column row**

```typescript
expect(formatTraceRowsText(trace)).toBe(
  "com.example.booking.BookingController.cancelBooking\t1\tsrc/main/java/com/example/booking/BookingController.java\t31",
);
```

- [ ] **Step 4: Run the focused tests and verify the expected failures**

Run: `pnpm vitest run tests/gatewayClient.test.ts tests/formatters.test.ts`

Expected: FAIL because the adapter ignores `callers`/`callees`, `TraceEdge` lacks `hop`, and the formatter still emits duplicate endpoints.

- [ ] **Step 5: Add hop to the normalized domain contract**

```typescript
export interface TraceEdge {
  sourceQualifiedName: string;
  targetQualifiedName: string;
  relatedQualifiedName: string;
  hop: number | null;
  filePath: string;
  line: number | null;
}
```

- [ ] **Step 6: Normalize current and legacy trace arrays in the adapter**

Replace the trace helpers with direction-aware versions:

```typescript
function traceRows(raw: unknown, direction: TraceDirection): Array<Record<string, unknown>> {
  if (!isRecord(raw)) return [];
  if (Array.isArray(raw.paths)) return raw.paths.filter(isRecord);

  const rows = direction === "inbound" ? raw.callers : raw.callees;
  return Array.isArray(rows) ? rows.filter(isRecord) : [];
}

function traceEdge(
  row: Record<string, unknown>,
  qualifiedName: string,
  direction: TraceDirection,
): TraceEdge {
  const currentRelated = firstString(row.qualified_name, row.qn, row.name);
  const sourceQualifiedName = firstString(
    row.sourceQualifiedName,
    row.source_qualified_name,
    row.source,
    row.caller,
    direction === "inbound" ? currentRelated : qualifiedName,
  );
  const targetQualifiedName = firstString(
    row.targetQualifiedName,
    row.target_qualified_name,
    row.target,
    row.callee,
    direction === "outbound" ? currentRelated : qualifiedName,
  );

  return {
    sourceQualifiedName,
    targetQualifiedName,
    relatedQualifiedName: direction === "inbound" ? sourceQualifiedName : targetQualifiedName,
    hop: firstNumber(row.hop),
    filePath: firstString(row.filePath, row.file_path, row.file, row.path),
    line: firstNumber(row.line, row.start_line),
  };
}
```

Pass `direction` into `traceRows(raw, direction)` before mapping.

- [ ] **Step 7: Compact the trace formatter**

```typescript
function formatTraceEdge(edge: TraceEdge): string {
  return [
    edge.relatedQualifiedName,
    fieldValue(edge.hop),
    edge.filePath,
    fieldValue(edge.line),
  ].join("\t");
}
```

- [ ] **Step 8: Run the focused tests and verify they pass**

Run: `pnpm vitest run tests/gatewayClient.test.ts tests/formatters.test.ts`

Expected: PASS.

- [ ] **Step 9: Commit the trace compatibility change**

```bash
git add tests/fixtures/codebaseMemory.ts tests/gatewayClient.test.ts tests/formatters.test.ts src/domain/types.ts src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts src/formatters/textFormatters.ts
git commit -m "fix: normalize live trace responses"
```

### Task 2: Bound Search and Standalone Trace Output

<TASK-ID>ATE-2</TASK-ID>

**Files:**
- Modify: `tests/cli.test.ts`
- Modify: `src/cli/createProgram.ts`

- [ ] **Step 1: Update trace test fixtures in `tests/cli.test.ts` with `hop` values**

Set `hop: 1` on every `TraceEdge` literal so it satisfies the Task 1 domain contract.

- [ ] **Step 2: Write a failing test for bounded defaults**

```typescript
it("uses context-safe search and trace defaults", async () => {
  const search = vi.fn().mockResolvedValue([]);
  const callers = vi.fn().mockResolvedValue([]);
  const { program } = createTestProgram(fakeClient({ callers, search }));

  await program.parseAsync(["node", "gcal", "search", "BookingService"]);
  await program.parseAsync([
    "node",
    "gcal",
    "callers",
    "com.example.BookingService.cancelBooking",
  ]);

  expect(search).toHaveBeenCalledWith("BookingService", {
    limit: 5,
    label: undefined,
    filePattern: undefined,
    qualifiedNamePattern: undefined,
  });
  expect(callers).toHaveBeenCalledWith("com.example.BookingService.cancelBooking", {
    depth: 1,
  });
});
```

- [ ] **Step 3: Write a failing test for standalone trace truncation**

```typescript
it("caps standalone traces and reports how to continue", async () => {
  const trace = Array.from({ length: 21 }, (_, index): TraceEdge => ({
    sourceQualifiedName: `com.example.Caller${index}`,
    targetQualifiedName: "com.example.Service.run",
    relatedQualifiedName: `com.example.Caller${index}`,
    hop: 1,
    filePath: `src/Caller${index}.ts`,
    line: index + 1,
  }));
  const callers = vi.fn().mockResolvedValue(trace);
  const { errors, program, writes } = createTestProgram(fakeClient({ callers }));

  await program.parseAsync(["node", "gcal", "callers", "com.example.Service.run"]);

  expect(writes.join("").trimEnd().split("\n")).toHaveLength(20);
  expect(errors.join("")).toBe(
    "gcal: callers truncated to 20 of 21 rows; rerun with --limit 21\n",
  );
});
```

- [ ] **Step 4: Run the CLI tests and verify both failures**

Run: `pnpm vitest run tests/cli.test.ts`

Expected: FAIL because search uses 20, trace depth uses 3, and traces are not capped.

- [ ] **Step 5: Add a trace limit option and one bounded-output helper**

```typescript
interface TraceCommandOptions {
  depth: number;
  limit: number;
}

function writeBoundedTrace(
  deps: ProgramDeps,
  command: "callers" | "callees",
  trace: TraceEdge[],
  limit: number,
): void {
  writeLine(deps.writeOut, formatTraceRowsText(trace.slice(0, limit)));
  if (trace.length > limit) {
    writeLine(
      deps.writeErr,
      `gcal: ${command} truncated to ${limit} of ${trace.length} rows; rerun with --limit ${trace.length}`,
    );
  }
}
```

Import `TraceEdge` as a type in `createProgram.ts`.

- [ ] **Step 6: Change CLI defaults and wire the helper**

Use `5` for search/symbol limits. Configure callers/callees with:

```typescript
.option("--depth <n>", "trace depth", numberOption, 1)
.option("--limit <n>", "maximum rows", numberOption, 20)
```

After each client call, invoke `writeBoundedTrace(deps, "callers", trace, options.limit)` or the callee equivalent.

- [ ] **Step 7: Add explicit override and invalid-limit coverage**

Extend the parameterized invalid-option cases with `--limit 1abc`, `--limit 1.5`, and `--limit -1`. Add one assertion that `--depth 2 --limit 1` passes depth 2 to the client and prints one row.

- [ ] **Step 8: Run the CLI tests and verify they pass**

Run: `pnpm vitest run tests/cli.test.ts`

Expected: PASS.

- [ ] **Step 9: Commit the bounded CLI defaults**

```bash
git add tests/cli.test.ts src/cli/createProgram.ts
git commit -m "perf: bound discovery command output"
```

### Task 3: Project Architecture onto a Bounded Contract

<TASK-ID>ATE-3</TASK-ID>

**Files:**
- Modify: `tests/fixtures/codebaseMemory.ts`
- Modify: `tests/gatewayClient.test.ts`
- Modify: `tests/normalize.test.ts`
- Modify: `src/adapters/codebaseMemoryMcp/mcpSchemas.ts`
- Modify: `src/adapters/codebaseMemoryMcp/normalize.ts`
- Modify: `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`

- [ ] **Step 1: Replace the architecture fixture with oversized and noisy data**

```typescript
export const architectureResponse = {
  project: "example-project",
  total_nodes: 1000,
  total_edges: 2000,
  languages: [{ name: "TypeScript", files: 20 }],
  packages: Array.from({ length: 21 }, (_, index) => ({ name: `package-${index}` })),
  entry_points: [{ qualified_name: "src.main" }],
  hotspots: [{ qualified_name: "src.hotspot" }],
  boundaries: [{ name: "adapter" }],
  layers: [{ name: "cli" }],
  clusters: [{ id: 1, label: "runtime" }],
  file_tree: Array.from({ length: 500 }, (_, index) => `src/file-${index}.ts`),
  routes: Array.from({ length: 50 }, (_, index) => ({ path: `/route-${index}` })),
};
```

- [ ] **Step 2: Write a failing normalization test**

```typescript
expect(normalizeArchitectureResponse(architectureResponse)).toEqual({
  project: "example-project",
  total_nodes: 1000,
  total_edges: 2000,
  languages: architectureResponse.languages,
  packages: architectureResponse.packages.slice(0, 20),
  entry_points: architectureResponse.entry_points,
  hotspots: architectureResponse.hotspots,
  boundaries: architectureResponse.boundaries,
  layers: architectureResponse.layers,
  clusters: architectureResponse.clusters,
});
```

Also assert that serialized output does not contain `file_tree` or `routes`.

- [ ] **Step 3: Write a failing gateway test for requested aspects and normalized output**

Assert `client.arch()` returns the projected response and that the gateway request contains:

```typescript
args: {
  project: "example-project",
  aspects: [
    "languages",
    "packages",
    "entry_points",
    "hotspots",
    "boundaries",
    "layers",
    "clusters",
  ],
}
```

- [ ] **Step 4: Run focused tests and verify the expected failures**

Run: `pnpm vitest run tests/normalize.test.ts tests/gatewayClient.test.ts`

Expected: FAIL because no architecture schema or normalizer exists and `arch()` forwards the raw payload.

- [ ] **Step 5: Add the raw architecture record schema**

```typescript
export const rawArchitectureResponseSchema = z.record(z.unknown());
```

- [ ] **Step 6: Implement the bounded architecture projection**

```typescript
const architectureScalarKeys = ["project", "total_nodes", "total_edges"] as const;
const architectureSectionKeys = [
  "languages",
  "packages",
  "entry_points",
  "hotspots",
  "boundaries",
  "layers",
  "clusters",
] as const;
const architectureSectionLimit = 20;

export function normalizeArchitectureResponse(raw: unknown): Record<string, unknown> {
  const parsed = rawArchitectureResponseSchema.parse(raw);
  const normalized: Record<string, unknown> = {};

  for (const key of architectureScalarKeys) {
    if (parsed[key] !== undefined) normalized[key] = parsed[key];
  }
  for (const key of architectureSectionKeys) {
    const value = parsed[key];
    if (Array.isArray(value)) normalized[key] = value.slice(0, architectureSectionLimit);
  }

  return normalized;
}
```

- [ ] **Step 7: Request only supported high-signal aspects and normalize the result**

```typescript
async arch(): Promise<unknown> {
  const raw = await this.invoke("codebase-memory-mcp::get_architecture", {
    project: this.config.project,
    aspects: [
      "languages",
      "packages",
      "entry_points",
      "hotspots",
      "boundaries",
      "layers",
      "clusters",
    ],
  });
  return normalizeArchitectureResponse(raw);
}
```

- [ ] **Step 8: Run focused tests and verify they pass**

Run: `pnpm vitest run tests/normalize.test.ts tests/gatewayClient.test.ts`

Expected: PASS.

- [ ] **Step 9: Commit the architecture projection**

```bash
git add tests/fixtures/codebaseMemory.ts tests/gatewayClient.test.ts tests/normalize.test.ts src/adapters/codebaseMemoryMcp/mcpSchemas.ts src/adapters/codebaseMemoryMcp/normalize.ts src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts
git commit -m "perf: bound architecture context"
```

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

### Task 5: Document Contracts and Verify the Complete Change

<TASK-ID>ATE-5</TASK-ID>

**Files:**
- Modify: `tests/installScript.test.ts`
- Modify: `README.md`
- Modify: `docs/superfastpowers/specs/2026-07-10-gcal-token-efficiency-design.md`

- [ ] **Step 1: Write a failing README contract assertion**

```typescript
expect(readme).toContain("sole model-facing code-discovery surface");
expect(readme).toContain("defaults to 5 candidates");
expect(readme).toContain("defaults to depth 1 and 20 rows");
expect(readme).toContain("omits the full file tree");
```

- [ ] **Step 2: Run the installer test and verify it fails**

Run: `pnpm vitest run tests/installScript.test.ts`

Expected: FAIL because README does not yet describe the new contracts.

- [ ] **Step 3: Update README command and workflow documentation**

Document these exact behaviors:

- search/symbol default to 5 candidates;
- callers/callees default to depth 1 and 20 rows, with `--depth` and `--limit` overrides;
- relationship rows are `related qualified name`, `hop`, `file`, `line`;
- architecture output is projected and omits the full file tree;
- GCAL is the sole model-facing code-discovery surface while available;
- the Windows installer migrates the old direct-MCP managed instruction block;
- exact-source tasks skip inspect and call get directly.

- [ ] **Step 4: Update the design spec only if implementation names differ**

Compare the implemented option names, output columns, architecture sections, and installer marker behavior with the spec. If any implemented name differs, edit the spec to use the shipped name; otherwise leave the spec unchanged.

- [ ] **Step 5: Run formatting checks on changed files**

Run: `pnpm exec prettier --check src tests workflow README.md docs/superfastpowers/specs/2026-07-10-gcal-token-efficiency-design.md`

Expected: PASS. If formatting fails, run `pnpm exec prettier --write` on only the reported changed files and rerun the check.

- [ ] **Step 6: Run the complete project verification**

Run: `pnpm check`

Expected: ESLint passes, all Vitest tests pass, and TypeScript build passes.

- [ ] **Step 7: Inspect the final diff for accidental scope expansion**

Run: `git diff --check && git status --short && git diff --stat main...HEAD`

Expected: no whitespace errors; only the files listed in this plan are changed; no `gcal elect`, batching, telemetry, generated build output, or live MCP test dependency is present.

- [ ] **Step 8: Commit documentation and verification updates**

```bash
git add README.md tests/installScript.test.ts docs/superfastpowers/specs/2026-07-10-gcal-token-efficiency-design.md
git commit -m "docs: explain bounded GCAL workflows"
```
