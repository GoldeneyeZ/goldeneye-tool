# Goldeneye Code Agent Layer Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Phase 1 of Goldeneye Code Agent Layer: deterministic context-election CLI primitives around `codebase-memory-mcp`, plus reusable Codex/Claude workflow assets.

**Architecture:** GCAL is a TypeScript CLI with a narrow adapter boundary around `codebase-memory-mcp`. The CLI commands call an GCAL-owned client interface, normalize variable MCP response shapes into stable domain types, apply small context-safety policies, and format compact agent-facing output.

**Tech Stack:** TypeScript, Node.js, pnpm, Commander, Zod, Vitest, ESLint, Prettier.

---

## Scope

Implement these commands:

```bash
gcal search <query> [--limit n] [--label label] [--file regex] [--qn regex]
gcal symbol <name-regex> [--limit n] [--label label] [--file regex] [--qn regex]
gcal inspect <query-or-qualified-name> [--limit n]
gcal get <qualified-name>
gcal callers <qualified-name> [--depth n]
gcal callees <qualified-name> [--depth n]
gcal arch
gcal status
gcal index [repo-path]
```

Do not implement `gcal elect`.

Use this first transport:

- HTTP JSON-RPC call to a gateway-compatible MCP endpoint.
- Default URL: `GCAL_MCP_URL=http://localhost:8767/mcp`.
- Default project: `GCAL_PROJECT` is required for commands that query an indexed project.
- Tool IDs use `codebase-memory-mcp::<tool-name>`.

Keep the transport behind `CodebaseMemoryClient` so direct MCP or a bundled server can replace it without changing CLI commands.

## File Structure

Create or modify these files:

```text
package.json
pnpm-lock.yaml
tsconfig.json
vitest.config.ts
eslint.config.js
.prettierrc.json
.gitignore
README.md
src/main.ts
src/cli/createProgram.ts
src/cli/output.ts
src/adapters/codebaseMemoryMcp/CodebaseMemoryClient.ts
src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts
src/adapters/codebaseMemoryMcp/gatewayJsonRpc.ts
src/adapters/codebaseMemoryMcp/mcpSchemas.ts
src/adapters/codebaseMemoryMcp/normalize.ts
src/domain/types.ts
src/kernel/affordanceSignals.ts
src/kernel/inspectPolicy.ts
src/kernel/tracePolicy.ts
src/formatters/textFormatters.ts
src/formatters/jsonFormatters.ts
src/workflows/createWorkflowFiles.ts
workflow/AGENTS.md
workflow/skills/codebase-memory/SKILL.md
workflow/skills/codebase-memory/agents/openai.yaml
workflow/skills/codebase-memory/agents/claude.md
tests/fixtures/codebaseMemory.ts
tests/normalize.test.ts
tests/formatters.test.ts
tests/policies.test.ts
tests/gatewayClient.test.ts
tests/cli.test.ts
```

Responsibilities:

- `src/main.ts`: executable CLI entrypoint.
- `src/cli/createProgram.ts`: Commander command definitions with dependency injection for tests.
- `src/cli/output.ts`: stdout/stderr writing and exit-code handling.
- `CodebaseMemoryClient.ts`: GCAL-owned interface and command argument types.
- `GatewayCodebaseMemoryClient.ts`: production adapter using JSON-RPC gateway calls.
- `gatewayJsonRpc.ts`: raw JSON-RPC request/response mechanics.
- `mcpSchemas.ts`: Zod schemas for raw MCP response shapes.
- `normalize.ts`: raw response conversion into stable GCAL domain types.
- `src/domain/types.ts`: normalized types shared by kernel, formatters, and CLI.
- `src/kernel/*`: thresholding, trace hints, and soft context-affordance warnings.
- `src/formatters/*`: compact text and JSON output contracts.
- `src/workflows/createWorkflowFiles.ts`: source strings for workflow-kit files, used by docs/tests if needed.
- `workflow/*`: installable workflow kit assets.
- `tests/fixtures/codebaseMemory.ts`: recorded representative responses.

## Task 1: Project Scaffold

**Files:**
- Create: `package.json`
- Create: `tsconfig.json`
- Create: `vitest.config.ts`
- Create: `eslint.config.js`
- Create: `.prettierrc.json`
- Create: `.gitignore`

- [ ] **Step 1: Create package metadata**

Create `package.json`:

```json
{
  "name": "goldeneye-code-agent-layer",
  "version": "0.1.0",
  "description": "Context-election CLI and workflow kit for agent users.",
  "type": "module",
  "bin": {
    "gcal": "./dist/main.js"
  },
  "scripts": {
    "build": "tsc -p tsconfig.json",
    "test": "vitest run",
    "test:watch": "vitest",
    "lint": "eslint .",
    "format": "prettier --write .",
    "check": "pnpm lint && pnpm test && pnpm build"
  },
  "dependencies": {
    "commander": "^12.1.0",
    "zod": "^3.23.8"
  },
  "devDependencies": {
    "@eslint/js": "^9.8.0",
    "@types/node": "^22.0.0",
    "eslint": "^9.8.0",
    "prettier": "^3.3.3",
    "typescript": "^5.5.4",
    "typescript-eslint": "^8.0.0",
    "vitest": "^2.0.5"
  },
  "engines": {
    "node": ">=20"
  }
}
```

- [ ] **Step 2: Create TypeScript config**

Create `tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "esModuleInterop": true,
    "forceConsistentCasingInFileNames": true,
    "skipLibCheck": true,
    "outDir": "dist",
    "rootDir": ".",
    "types": ["node"]
  },
  "include": ["src/**/*.ts", "tests/**/*.ts", "vitest.config.ts", "eslint.config.js"]
}
```

- [ ] **Step 3: Create test config**

Create `vitest.config.ts`:

```ts
import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["tests/**/*.test.ts"],
    clearMocks: true,
  },
});
```

- [ ] **Step 4: Create lint and format config**

Create `eslint.config.js`:

```js
import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default [
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    ignores: ["dist/**", "coverage/**"],
  },
];
```

Create `.prettierrc.json`:

```json
{
  "printWidth": 100,
  "semi": true,
  "trailingComma": "all"
}
```

Create `.gitignore`:

```gitignore
node_modules/
dist/
coverage/
.env
*.log
```

- [ ] **Step 5: Install dependencies**

Run:

```bash
pnpm install
```

Expected: `pnpm-lock.yaml` is created and dependencies install successfully.

- [ ] **Step 6: Run initial checks**

Run:

```bash
pnpm test
pnpm build
```

Expected: `pnpm test` reports no test files or passes with zero tests, and `pnpm build` succeeds once source files exist in later tasks. If `tsc` fails because no source file exists yet, continue to Task 2 before re-running `pnpm build`.

- [ ] **Step 7: Commit scaffold**

```bash
git add package.json pnpm-lock.yaml tsconfig.json vitest.config.ts eslint.config.js .prettierrc.json .gitignore
git commit -m "chore: scaffold TypeScript CLI project"
```

## Task 2: Domain Types And Fixtures

**Files:**
- Create: `src/domain/types.ts`
- Create: `tests/fixtures/codebaseMemory.ts`
- Test: `tests/normalize.test.ts`

- [ ] **Step 1: Write fixture data**

Create `tests/fixtures/codebaseMemory.ts`:

```ts
export const searchGraphResponse = {
  results: [
    {
      qualified_name: "com.example.booking.BookingService.cancelBooking",
      label: "Method",
      file_path: "src/main/java/com/example/booking/BookingService.java",
      start_line: 42,
      signature: "public BookingResponse cancelBooking(String bookingId)",
    },
  ],
};

export const methodSnippetResponse = {
  qualified_name: "com.example.booking.BookingService.cancelBooking",
  label: "Method",
  file_path: "src/main/java/com/example/booking/BookingService.java",
  start_line: 42,
  end_line: 58,
  lines: 17,
  complexity: 3,
  cognitive: 4,
  signature: "public BookingResponse cancelBooking(String bookingId)",
  return_type: "BookingResponse",
  callers: 4,
  callees: 2,
  code: [
    "public BookingResponse cancelBooking(String bookingId) {",
    "    Booking booking = resolveActiveBooking(bookingId);",
    "    booking.cancel();",
    "    return BookingResponse.from(booking);",
    "}",
  ].join("\n"),
};

export const largeMethodSnippetResponse = {
  ...methodSnippetResponse,
  qualified_name: "com.example.booking.BookingService.reconcileBooking",
  lines: 95,
  complexity: 12,
  cognitive: 21,
  callers: 12,
  callees: 9,
};

export const inboundTraceResponse = {
  paths: [
    {
      caller: "com.example.booking.BookingController.cancelBooking",
      file_path: "src/main/java/com/example/booking/BookingController.java",
      start_line: 31,
      callee: "com.example.booking.BookingService.cancelBooking",
    },
  ],
};

export const architectureResponse = {
  project: "example-project",
  modules: [{ name: "booking", files: 12 }],
};

export const statusResponse = {
  project: "example-project",
  indexed: true,
  symbols: 423,
};
```

- [ ] **Step 2: Create domain types**

Create `src/domain/types.ts`:

```ts
export type SymbolKind = "Class" | "Method" | "Function" | "Field" | "Unknown";

export interface SymbolCandidate {
  qualifiedName: string;
  label: SymbolKind | string;
  filePath: string;
  line: number | null;
  signature: string;
}

export interface SelectedSymbol {
  qualifiedName: string;
  kind: SymbolKind | string;
  filePath: string;
  startLine: number | null;
  endLine: number | null;
  lines: number | null;
  complexity: number | null;
  cognitive: number | null;
  visibility: string;
  signature: string;
  returnType: string;
  decorators: string;
  callers: number | null;
  callees: number | null;
  source: string;
}

export interface TraceEdge {
  qualifiedName: string;
  filePath: string;
  line: number | null;
}

export interface InspectResult {
  candidates: SymbolCandidate[];
  selected: SelectedSymbol;
  inbound: TraceEdge[] | TraceHint;
  outbound: TraceEdge[] | TraceHint;
  warnings: string[];
}

export interface TraceHint {
  kind: "hint";
  count: number;
  command: string;
}

export interface SearchOptions {
  limit: number;
  label?: string;
  filePattern?: string;
  qualifiedNamePattern?: string;
}

export interface InspectOptions {
  limit: number;
}

export interface TraceOptions {
  depth: number;
}
```

- [ ] **Step 3: Commit domain setup**

```bash
git add src/domain/types.ts tests/fixtures/codebaseMemory.ts
git commit -m "feat: define GCAL domain types"
```

## Task 3: Normalize MCP Responses

**Files:**
- Create: `src/adapters/codebaseMemoryMcp/mcpSchemas.ts`
- Create: `src/adapters/codebaseMemoryMcp/normalize.ts`
- Test: `tests/normalize.test.ts`

- [ ] **Step 1: Write failing normalizer tests**

Create `tests/normalize.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import {
  largeMethodSnippetResponse,
  methodSnippetResponse,
  searchGraphResponse,
} from "./fixtures/codebaseMemory.js";
import {
  normalizeSearchResponse,
  normalizeSelectedSymbol,
} from "../src/adapters/codebaseMemoryMcp/normalize.js";

describe("codebase-memory response normalization", () => {
  it("normalizes search results into compact candidates", () => {
    expect(normalizeSearchResponse(searchGraphResponse)).toEqual([
      {
        qualifiedName: "com.example.booking.BookingService.cancelBooking",
        label: "Method",
        filePath: "src/main/java/com/example/booking/BookingService.java",
        line: 42,
        signature: "public BookingResponse cancelBooking(String bookingId)",
      },
    ]);
  });

  it("normalizes selected symbol metadata and source", () => {
    expect(normalizeSelectedSymbol(methodSnippetResponse)).toMatchObject({
      qualifiedName: "com.example.booking.BookingService.cancelBooking",
      kind: "Method",
      filePath: "src/main/java/com/example/booking/BookingService.java",
      startLine: 42,
      endLine: 58,
      lines: 17,
      complexity: 3,
      cognitive: 4,
      signature: "public BookingResponse cancelBooking(String bookingId)",
      returnType: "BookingResponse",
      callers: 4,
      callees: 2,
    });
  });

  it("keeps large symbols as metadata without dropping source for get commands", () => {
    const selected = normalizeSelectedSymbol(largeMethodSnippetResponse);
    expect(selected.lines).toBe(95);
    expect(selected.source).toContain("cancelBooking");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
pnpm vitest run tests/normalize.test.ts
```

Expected: FAIL because `normalize.ts` does not exist.

- [ ] **Step 3: Create raw schemas**

Create `src/adapters/codebaseMemoryMcp/mcpSchemas.ts`:

```ts
import { z } from "zod";

export const rawSearchItemSchema = z.object({
  qualified_name: z.string().optional(),
  qn: z.string().optional(),
  name: z.string().optional(),
  label: z.string().optional(),
  type: z.string().optional(),
  labels: z.array(z.string()).optional(),
  file_path: z.string().optional(),
  file: z.string().optional(),
  path: z.string().optional(),
  start_line: z.number().optional(),
  line: z.number().optional(),
  signature: z.string().optional(),
});

export const rawSearchResponseSchema = z
  .object({
    results: z.array(rawSearchItemSchema).optional(),
    semantic_results: z.array(rawSearchItemSchema).optional(),
    matches: z.array(rawSearchItemSchema).optional(),
  })
  .passthrough();

export const rawSnippetSchema = z
  .object({
    qualified_name: z.string().optional(),
    qn: z.string().optional(),
    name: z.string().optional(),
    label: z.string().optional(),
    type: z.string().optional(),
    file_path: z.string().optional(),
    file: z.string().optional(),
    path: z.string().optional(),
    start_line: z.number().optional(),
    line: z.number().optional(),
    end_line: z.number().optional(),
    lines: z.number().optional(),
    complexity: z.number().optional(),
    cognitive: z.number().optional(),
    signature: z.string().optional(),
    return_type: z.string().optional(),
    decorators: z.string().optional(),
    callers: z.number().optional(),
    callees: z.number().optional(),
    code: z.string().optional(),
    source: z.string().optional(),
    snippet: z.string().optional(),
    content: z.string().optional(),
    text: z.string().optional(),
  })
  .passthrough();
```

- [ ] **Step 4: Implement normalizers**

Create `src/adapters/codebaseMemoryMcp/normalize.ts`:

```ts
import type { SelectedSymbol, SymbolCandidate } from "../../domain/types.js";
import { rawSearchResponseSchema, rawSnippetSchema } from "./mcpSchemas.js";

function firstString(...values: Array<unknown>): string {
  for (const value of values) {
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return "";
}

function firstNumber(...values: Array<unknown>): number | null {
  for (const value of values) {
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return null;
}

export function normalizeSearchResponse(raw: unknown): SymbolCandidate[] {
  const parsed = rawSearchResponseSchema.parse(raw);
  const rows = [
    ...(parsed.results ?? []),
    ...(parsed.semantic_results ?? []),
    ...(parsed.matches ?? []),
  ];

  return rows.map((row) => ({
    qualifiedName: firstString(row.qualified_name, row.qn, row.name),
    label: Array.isArray(row.labels) ? row.labels.join(",") : firstString(row.label, row.type),
    filePath: firstString(row.file_path, row.file, row.path),
    line: firstNumber(row.start_line, row.line),
    signature: firstString(row.signature),
  }));
}

export function normalizeSelectedSymbol(raw: unknown): SelectedSymbol {
  const parsed = rawSnippetSchema.parse(raw);

  return {
    qualifiedName: firstString(parsed.qualified_name, parsed.qn, parsed.name),
    kind: firstString(parsed.label, parsed.type, "Unknown"),
    filePath: firstString(parsed.file_path, parsed.file, parsed.path),
    startLine: firstNumber(parsed.start_line, parsed.line),
    endLine: firstNumber(parsed.end_line),
    lines: firstNumber(parsed.lines),
    complexity: firstNumber(parsed.complexity),
    cognitive: firstNumber(parsed.cognitive),
    visibility: "",
    signature: firstString(parsed.signature),
    returnType: firstString(parsed.return_type),
    decorators: firstString(parsed.decorators),
    callers: firstNumber(parsed.callers),
    callees: firstNumber(parsed.callees),
    source: firstString(parsed.code, parsed.source, parsed.snippet, parsed.content, parsed.text),
  };
}
```

- [ ] **Step 5: Run normalizer tests**

Run:

```bash
pnpm vitest run tests/normalize.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit normalizers**

```bash
git add src/adapters/codebaseMemoryMcp/mcpSchemas.ts src/adapters/codebaseMemoryMcp/normalize.ts tests/normalize.test.ts
git commit -m "feat: normalize codebase-memory responses"
```

## Task 4: Formatting Contracts

**Files:**
- Create: `src/formatters/textFormatters.ts`
- Create: `src/formatters/jsonFormatters.ts`
- Test: `tests/formatters.test.ts`

- [ ] **Step 1: Write formatter tests**

Create `tests/formatters.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { methodSnippetResponse, searchGraphResponse } from "./fixtures/codebaseMemory.js";
import {
  formatCandidatesText,
  formatSelectedMetadataText,
  formatSourceText,
} from "../src/formatters/textFormatters.js";
import { normalizeSearchResponse, normalizeSelectedSymbol } from "../src/adapters/codebaseMemoryMcp/normalize.js";

describe("text formatters", () => {
  it("formats search candidates as tab-separated rows", () => {
    const rows = normalizeSearchResponse(searchGraphResponse);
    expect(formatCandidatesText(rows)).toBe(
      "com.example.booking.BookingService.cancelBooking\tMethod\tsrc/main/java/com/example/booking/BookingService.java\t42\tpublic BookingResponse cancelBooking(String bookingId)",
    );
  });

  it("formats inspect metadata without source", () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const output = formatSelectedMetadataText(selected, []);
    expect(output).toContain("# selected");
    expect(output).toContain("qualified_name=com.example.booking.BookingService.cancelBooking");
    expect(output).not.toContain("resolveActiveBooking");
  });

  it("formats get output as source only", () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    expect(formatSourceText(selected)).toBe(selected.source);
  });
});
```

- [ ] **Step 2: Run formatter tests to verify failure**

Run:

```bash
pnpm vitest run tests/formatters.test.ts
```

Expected: FAIL because formatter files do not exist.

- [ ] **Step 3: Implement text formatters**

Create `src/formatters/textFormatters.ts`:

```ts
import type { SelectedSymbol, SymbolCandidate, TraceEdge, TraceHint } from "../domain/types.js";

function lineValue(value: string | number | null): string {
  return value === null ? "" : String(value);
}

export function formatCandidatesText(candidates: SymbolCandidate[]): string {
  return candidates
    .map((candidate) =>
      [
        candidate.qualifiedName,
        candidate.label,
        candidate.filePath,
        lineValue(candidate.line),
        candidate.signature,
      ].join("\t"),
    )
    .join("\n");
}

export function formatCandidateBlockText(candidates: SymbolCandidate[]): string {
  if (candidates.length === 0) {
    return "# candidates\n";
  }
  return [
    "# candidates",
    ...candidates.map((candidate, index) =>
      [
        String(index + 1),
        candidate.label,
        candidate.qualifiedName,
        `${candidate.filePath}:${lineValue(candidate.line)}`,
        candidate.signature,
      ].join("\t"),
    ),
  ].join("\n");
}

export function formatSelectedMetadataText(selected: SelectedSymbol, warnings: string[]): string {
  const lines = [
    "# selected",
    `qualified_name=${selected.qualifiedName}`,
    `kind=${selected.kind}`,
    `file=${selected.filePath}`,
    `line=${lineValue(selected.startLine)}`,
    `lines=${lineValue(selected.lines)}`,
    `complexity=${lineValue(selected.complexity)}`,
    `cognitive=${lineValue(selected.cognitive)}`,
    `visibility=${selected.visibility}`,
    `signature=${selected.signature}`,
    `return_type=${selected.returnType}`,
    `decorators=${selected.decorators}`,
    `callers=${lineValue(selected.callers)}`,
    `callees=${lineValue(selected.callees)}`,
  ];

  if (warnings.length > 0) {
    lines.push("", "# warnings", ...warnings.map((warning) => `warning=${warning}`));
  }

  return lines.join("\n");
}

export function formatTraceSectionText(title: string, trace: TraceEdge[] | TraceHint): string {
  if (!Array.isArray(trace)) {
    return [`# ${title}`, `${trace.count} relationships; use: ${trace.command}`].join("\n");
  }
  return [
    `# ${title}`,
    ...trace.map((edge) => [edge.qualifiedName, `${edge.filePath}:${lineValue(edge.line)}`].join("\t")),
  ].join("\n");
}

export function formatSourceText(selected: SelectedSymbol): string {
  return selected.source;
}
```

- [ ] **Step 4: Implement JSON formatter**

Create `src/formatters/jsonFormatters.ts`:

```ts
export function formatCompactJson(value: unknown): string {
  return JSON.stringify(value);
}
```

- [ ] **Step 5: Run formatter tests**

Run:

```bash
pnpm vitest run tests/formatters.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit formatters**

```bash
git add src/formatters/textFormatters.ts src/formatters/jsonFormatters.ts tests/formatters.test.ts
git commit -m "feat: add compact output formatters"
```

## Task 5: Context-Safety Policies

**Files:**
- Create: `src/kernel/affordanceSignals.ts`
- Create: `src/kernel/tracePolicy.ts`
- Create: `src/kernel/inspectPolicy.ts`
- Test: `tests/policies.test.ts`

- [ ] **Step 1: Write policy tests**

Create `tests/policies.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { largeMethodSnippetResponse, methodSnippetResponse } from "./fixtures/codebaseMemory.js";
import { normalizeSelectedSymbol } from "../src/adapters/codebaseMemoryMcp/normalize.js";
import { contextAffordanceWarnings } from "../src/kernel/affordanceSignals.js";
import { inboundTraceDecision } from "../src/kernel/tracePolicy.js";

describe("context-safety policies", () => {
  it("does not warn for compact explicit methods", () => {
    expect(contextAffordanceWarnings(normalizeSelectedSymbol(methodSnippetResponse))).toEqual([]);
  });

  it("warns when large methods reduce context affordance", () => {
    expect(contextAffordanceWarnings(normalizeSelectedSymbol(largeMethodSnippetResponse))).toContain(
      "large method; source likely needed",
    );
  });

  it("returns trace hint when caller count exceeds threshold", () => {
    expect(
      inboundTraceDecision({
        qualifiedName: "com.example.booking.BookingService.reconcileBooking",
        callerCount: 12,
        threshold: 5,
      }),
    ).toEqual({
      kind: "hint",
      count: 12,
      command:
        "gcal callers com.example.booking.BookingService.reconcileBooking --depth 1",
    });
  });
});
```

- [ ] **Step 2: Run policy tests to verify failure**

Run:

```bash
pnpm vitest run tests/policies.test.ts
```

Expected: FAIL because policy files do not exist.

- [ ] **Step 3: Implement affordance warnings**

Create `src/kernel/affordanceSignals.ts`:

```ts
import type { SelectedSymbol } from "../domain/types.js";

export function contextAffordanceWarnings(selected: SelectedSymbol): string[] {
  const warnings: string[] = [];

  if ((selected.lines ?? 0) >= 80) {
    warnings.push("large method; source likely needed");
  }

  if ((selected.complexity ?? 0) >= 10 || (selected.cognitive ?? 0) >= 15) {
    warnings.push("high complexity; inspect related callers and tests before editing");
  }

  if ((selected.callers ?? 0) > 8) {
    warnings.push("high caller count; use callers command rather than inline trace");
  }

  return warnings;
}
```

- [ ] **Step 4: Implement trace policy**

Create `src/kernel/tracePolicy.ts`:

```ts
import type { TraceHint } from "../domain/types.js";

export interface TraceDecisionInput {
  qualifiedName: string;
  callerCount: number | null;
  threshold: number;
}

export function inboundTraceDecision(input: TraceDecisionInput): TraceHint | null {
  const count = input.callerCount ?? 0;
  if (count <= input.threshold) {
    return null;
  }

  return {
    kind: "hint",
    count,
    command: `gcal callers ${input.qualifiedName} --depth 1`,
  };
}
```

- [ ] **Step 5: Implement inspect policy**

Create `src/kernel/inspectPolicy.ts`:

```ts
export const DEFAULT_CALLER_TRACE_THRESHOLD = 5;

export function callerTraceThresholdFromEnv(env: NodeJS.ProcessEnv): number {
  const raw = env.GCAL_CALLER_TRACE_THRESHOLD;
  if (raw === undefined || raw.trim() === "") {
    return DEFAULT_CALLER_TRACE_THRESHOLD;
  }

  const parsed = Number.parseInt(raw, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return DEFAULT_CALLER_TRACE_THRESHOLD;
  }

  return parsed;
}
```

- [ ] **Step 6: Run policy tests**

Run:

```bash
pnpm vitest run tests/policies.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit policies**

```bash
git add src/kernel/affordanceSignals.ts src/kernel/tracePolicy.ts src/kernel/inspectPolicy.ts tests/policies.test.ts
git commit -m "feat: add context-safety policies"
```

## Task 6: Gateway Adapter

**Files:**
- Create: `src/adapters/codebaseMemoryMcp/CodebaseMemoryClient.ts`
- Create: `src/adapters/codebaseMemoryMcp/gatewayJsonRpc.ts`
- Create: `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`
- Test: `tests/gatewayClient.test.ts`

- [ ] **Step 1: Write adapter tests**

Create `tests/gatewayClient.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { GatewayCodebaseMemoryClient } from "../src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.js";
import { searchGraphResponse } from "./fixtures/codebaseMemory.js";

describe("GatewayCodebaseMemoryClient", () => {
  it("invokes codebase-memory search_graph through gateway.invoke", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({
        result: {
          content: [{ text: JSON.stringify(searchGraphResponse) }],
        },
      }),
    });

    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.search("BookingService", { limit: 5 });
    expect(result[0]?.qualifiedName).toBe("com.example.booking.BookingService.cancelBooking");
    expect(fetchMock).toHaveBeenCalledOnce();
  });
});
```

- [ ] **Step 2: Run adapter test to verify failure**

Run:

```bash
pnpm vitest run tests/gatewayClient.test.ts
```

Expected: FAIL because adapter files do not exist.

- [ ] **Step 3: Define client interface**

Create `src/adapters/codebaseMemoryMcp/CodebaseMemoryClient.ts`:

```ts
import type {
  SearchOptions,
  SelectedSymbol,
  SymbolCandidate,
  TraceEdge,
  TraceOptions,
} from "../../domain/types.js";

export interface CodebaseMemoryClient {
  search(query: string, options: Partial<SearchOptions>): Promise<SymbolCandidate[]>;
  symbol(nameRegex: string, options: Partial<SearchOptions>): Promise<SymbolCandidate[]>;
  get(qualifiedName: string): Promise<SelectedSymbol>;
  callers(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]>;
  callees(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]>;
  arch(): Promise<unknown>;
  status(): Promise<unknown>;
  index(repoPath: string): Promise<unknown>;
}
```

- [ ] **Step 4: Implement JSON-RPC helper**

Create `src/adapters/codebaseMemoryMcp/gatewayJsonRpc.ts`:

```ts
export interface GatewayInvokeInput {
  mcpUrl: string;
  toolId: string;
  args: Record<string, unknown>;
  fetch: typeof globalThis.fetch;
}

function parsePossiblyJson(value: unknown): unknown {
  if (typeof value !== "string") {
    return value;
  }
  try {
    return parsePossiblyJson(JSON.parse(value));
  } catch {
    return value;
  }
}

export async function gatewayInvoke(input: GatewayInvokeInput): Promise<unknown> {
  const response = await input.fetch(input.mcpUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: {
        name: "gateway.invoke",
        arguments: {
          id: input.toolId,
          args: input.args,
        },
      },
    }),
  });

  if (!response.ok) {
    throw new Error(`MCP request failed with HTTP ${response.status}`);
  }

  const json = await response.json();
  if (json.error) {
    throw new Error(`MCP error: ${json.error.message ?? JSON.stringify(json.error)}`);
  }

  const text = json.result?.content?.[0]?.text;
  if (text === undefined) {
    throw new Error("MCP response did not include result content text");
  }

  return parsePossiblyJson(text);
}
```

- [ ] **Step 5: Implement gateway-backed client**

Create `src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts`:

```ts
import type {
  SearchOptions,
  TraceEdge,
  TraceOptions,
} from "../../domain/types.js";
import type { CodebaseMemoryClient } from "./CodebaseMemoryClient.js";
import { gatewayInvoke } from "./gatewayJsonRpc.js";
import { normalizeSearchResponse, normalizeSelectedSymbol } from "./normalize.js";

export interface GatewayClientConfig {
  mcpUrl: string;
  project: string;
  fetch?: typeof globalThis.fetch;
}

export class GatewayCodebaseMemoryClient implements CodebaseMemoryClient {
  private readonly fetchImpl: typeof globalThis.fetch;

  constructor(private readonly config: GatewayClientConfig) {
    this.fetchImpl = config.fetch ?? globalThis.fetch;
  }

  async search(query: string, options: Partial<SearchOptions>) {
    const raw = await this.invoke("codebase-memory-mcp::search_graph", {
      project: this.config.project,
      query,
      limit: options.limit ?? 20,
      label: options.label,
      file_pattern: options.filePattern,
      qn_pattern: options.qualifiedNamePattern,
    });
    return normalizeSearchResponse(raw);
  }

  async symbol(nameRegex: string, options: Partial<SearchOptions>) {
    const raw = await this.invoke("codebase-memory-mcp::search_graph", {
      project: this.config.project,
      name_pattern: nameRegex,
      limit: options.limit ?? 20,
      label: options.label,
      file_pattern: options.filePattern,
      qn_pattern: options.qualifiedNamePattern,
    });
    return normalizeSearchResponse(raw);
  }

  async get(qualifiedName: string) {
    const raw = await this.invoke("codebase-memory-mcp::get_code_snippet", {
      project: this.config.project,
      qualified_name: qualifiedName,
    });
    return normalizeSelectedSymbol(raw);
  }

  async callers(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.trace(qualifiedName, "inbound", options.depth);
  }

  async callees(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.trace(qualifiedName, "outbound", options.depth);
  }

  async arch(): Promise<unknown> {
    return this.invoke("codebase-memory-mcp::get_architecture", { project: this.config.project });
  }

  async status(): Promise<unknown> {
    return this.invoke("codebase-memory-mcp::index_status", { project: this.config.project });
  }

  async index(repoPath: string): Promise<unknown> {
    return this.invoke("codebase-memory-mcp::index_repository", { repo_path: repoPath });
  }

  private async trace(
    qualifiedName: string,
    direction: "inbound" | "outbound",
    depth: number,
  ): Promise<TraceEdge[]> {
    const raw = await this.invoke("codebase-memory-mcp::trace_path", {
      project: this.config.project,
      function_name: qualifiedName,
      direction,
      depth,
      mode: "calls",
    });
    const rows = Array.isArray((raw as { paths?: unknown[] }).paths)
      ? ((raw as { paths: unknown[] }).paths)
      : [];
    return rows.map((row) => {
      const record = row as Record<string, unknown>;
      return {
        qualifiedName: String(
          record.qualified_name ?? record.caller ?? record.callee ?? record.name ?? "",
        ),
        filePath: String(record.file_path ?? record.file ?? record.path ?? ""),
        line: typeof record.start_line === "number" ? record.start_line : null,
      };
    });
  }

  private invoke(toolId: string, args: Record<string, unknown>): Promise<unknown> {
    return gatewayInvoke({
      mcpUrl: this.config.mcpUrl,
      toolId,
      args: Object.fromEntries(Object.entries(args).filter(([, value]) => value !== undefined)),
      fetch: this.fetchImpl,
    });
  }
}
```

- [ ] **Step 6: Run adapter tests**

Run:

```bash
pnpm vitest run tests/gatewayClient.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit adapter**

```bash
git add src/adapters/codebaseMemoryMcp/CodebaseMemoryClient.ts src/adapters/codebaseMemoryMcp/gatewayJsonRpc.ts src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.ts tests/gatewayClient.test.ts
git commit -m "feat: add codebase-memory gateway adapter"
```

## Task 7: CLI Commands

**Files:**
- Create: `src/cli/createProgram.ts`
- Create: `src/cli/output.ts`
- Create: `src/main.ts`
- Test: `tests/cli.test.ts`

- [ ] **Step 1: Write CLI tests**

Create `tests/cli.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import { createProgram } from "../src/cli/createProgram.js";
import { methodSnippetResponse, searchGraphResponse } from "./fixtures/codebaseMemory.js";
import { normalizeSearchResponse, normalizeSelectedSymbol } from "../src/adapters/codebaseMemoryMcp/normalize.js";

describe("GCAL CLI", () => {
  it("prints search rows", async () => {
    const writes: string[] = [];
    const program = createProgram({
      client: {
        search: vi.fn().mockResolvedValue(normalizeSearchResponse(searchGraphResponse)),
        symbol: vi.fn(),
        get: vi.fn(),
        callers: vi.fn(),
        callees: vi.fn(),
        arch: vi.fn(),
        status: vi.fn(),
        index: vi.fn(),
      },
      writeOut: (text) => writes.push(text),
      writeErr: () => undefined,
    });

    await program.parseAsync(["node", "gcal", "search", "BookingService", "--limit", "5"]);
    expect(writes.join("")).toContain("com.example.booking.BookingService.cancelBooking");
  });

  it("prints get output as source only", async () => {
    const writes: string[] = [];
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const program = createProgram({
      client: {
        search: vi.fn(),
        symbol: vi.fn(),
        get: vi.fn().mockResolvedValue(selected),
        callers: vi.fn(),
        callees: vi.fn(),
        arch: vi.fn(),
        status: vi.fn(),
        index: vi.fn(),
      },
      writeOut: (text) => writes.push(text),
      writeErr: () => undefined,
    });

    await program.parseAsync([
      "node",
      "gcal",
      "get",
      "com.example.booking.BookingService.cancelBooking",
    ]);
    expect(writes.join("")).toBe(`${selected.source}\n`);
  });
});
```

- [ ] **Step 2: Run CLI tests to verify failure**

Run:

```bash
pnpm vitest run tests/cli.test.ts
```

Expected: FAIL because CLI files do not exist.

- [ ] **Step 3: Implement output helpers**

Create `src/cli/output.ts`:

```ts
export type WriteFn = (text: string) => void;

export function writeLine(write: WriteFn, text: string): void {
  write(`${text}\n`);
}
```

- [ ] **Step 4: Implement command program**

Create `src/cli/createProgram.ts`:

```ts
import { Command } from "commander";
import type { CodebaseMemoryClient } from "../adapters/codebaseMemoryMcp/CodebaseMemoryClient.js";
import { formatCompactJson } from "../formatters/jsonFormatters.js";
import {
  formatCandidatesText,
  formatSourceText,
  formatTraceSectionText,
} from "../formatters/textFormatters.js";
import { writeLine, type WriteFn } from "./output.js";

export interface ProgramDeps {
  client: CodebaseMemoryClient;
  writeOut: WriteFn;
  writeErr: WriteFn;
}

function numberOption(value: string): number {
  const parsed = Number.parseInt(value, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    throw new Error(`Expected a non-negative number, got ${value}`);
  }
  return parsed;
}

export function createProgram(deps: ProgramDeps): Command {
  const program = new Command();
  program.name("gcal").description("Goldeneye Code Agent Layer").showHelpAfterError();

  program
    .command("search")
    .argument("<query>")
    .option("--limit <n>", "maximum rows", numberOption, 20)
    .option("--label <label>")
    .option("--file <regex>")
    .option("--qn <regex>")
    .action(async (query: string, options) => {
      const rows = await deps.client.search(query, {
        limit: options.limit,
        label: options.label,
        filePattern: options.file,
        qualifiedNamePattern: options.qn,
      });
      writeLine(deps.writeOut, formatCandidatesText(rows));
    });

  program
    .command("symbol")
    .argument("<nameRegex>")
    .option("--limit <n>", "maximum rows", numberOption, 20)
    .option("--label <label>")
    .option("--file <regex>")
    .option("--qn <regex>")
    .action(async (nameRegex: string, options) => {
      const rows = await deps.client.symbol(nameRegex, {
        limit: options.limit,
        label: options.label,
        filePattern: options.file,
        qualifiedNamePattern: options.qn,
      });
      writeLine(deps.writeOut, formatCandidatesText(rows));
    });

  program.command("get").argument("<qualifiedName>").action(async (qualifiedName: string) => {
    const selected = await deps.client.get(qualifiedName);
    writeLine(deps.writeOut, formatSourceText(selected));
  });

  program
    .command("callers")
    .argument("<qualifiedName>")
    .option("--depth <n>", "trace depth", numberOption, 3)
    .action(async (qualifiedName: string, options) => {
      const trace = await deps.client.callers(qualifiedName, { depth: options.depth });
      writeLine(deps.writeOut, formatTraceSectionText("inbound", trace));
    });

  program
    .command("callees")
    .argument("<qualifiedName>")
    .option("--depth <n>", "trace depth", numberOption, 3)
    .action(async (qualifiedName: string, options) => {
      const trace = await deps.client.callees(qualifiedName, { depth: options.depth });
      writeLine(deps.writeOut, formatTraceSectionText("outbound", trace));
    });

  program.command("arch").action(async () => {
    writeLine(deps.writeOut, formatCompactJson(await deps.client.arch()));
  });

  program.command("status").action(async () => {
    writeLine(deps.writeOut, formatCompactJson(await deps.client.status()));
  });

  program.command("index").argument("[repoPath]", ".", { defaultValue: "." }).action(async (repoPath: string) => {
    writeLine(deps.writeOut, formatCompactJson(await deps.client.index(repoPath)));
  });

  return program;
}
```

- [ ] **Step 5: Implement executable entrypoint**

Create `src/main.ts`:

```ts
#!/usr/bin/env node
import { GatewayCodebaseMemoryClient } from "./adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.js";
import { createProgram } from "./cli/createProgram.js";

const mcpUrl = process.env.GCAL_MCP_URL ?? "http://localhost:8767/mcp";
const project = process.env.GCAL_PROJECT;

if (!project) {
  process.stderr.write("GCAL_PROJECT is required for codebase-memory commands\n");
  process.exit(2);
}

const client = new GatewayCodebaseMemoryClient({ mcpUrl, project });
const program = createProgram({
  client,
  writeOut: (text) => process.stdout.write(text),
  writeErr: (text) => process.stderr.write(text),
});

try {
  await program.parseAsync(process.argv);
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(`${message}\n`);
  process.exit(1);
}
```

- [ ] **Step 6: Run CLI tests**

Run:

```bash
pnpm vitest run tests/cli.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit CLI**

```bash
git add src/cli/createProgram.ts src/cli/output.ts src/main.ts tests/cli.test.ts
git commit -m "feat: add GCAL CLI commands"
```

## Task 8: Inspect Command Composition

**Files:**
- Modify: `src/cli/createProgram.ts`
- Modify: `src/formatters/textFormatters.ts`
- Test: `tests/cli.test.ts`

- [ ] **Step 1: Add inspect CLI test**

Append to `tests/cli.test.ts`:

```ts
it("prints inspect metadata without full source", async () => {
  const writes: string[] = [];
  const selected = normalizeSelectedSymbol(methodSnippetResponse);
  const program = createProgram({
    client: {
      search: vi.fn().mockResolvedValue(normalizeSearchResponse(searchGraphResponse)),
      symbol: vi.fn(),
      get: vi.fn().mockResolvedValue(selected),
      callers: vi.fn().mockResolvedValue([]),
      callees: vi.fn().mockResolvedValue([]),
      arch: vi.fn(),
      status: vi.fn(),
      index: vi.fn(),
    },
    writeOut: (text) => writes.push(text),
    writeErr: () => undefined,
  });

  await program.parseAsync([
    "node",
    "gcal",
    "inspect",
    "com.example.booking.BookingService.cancelBooking",
  ]);

  const output = writes.join("");
  expect(output).toContain("# selected");
  expect(output).toContain("# inbound");
  expect(output).not.toContain("resolveActiveBooking");
});
```

- [ ] **Step 2: Run test to verify failure**

Run:

```bash
pnpm vitest run tests/cli.test.ts
```

Expected: FAIL because the `inspect` command is not registered.

- [ ] **Step 3: Add inspect command**

Modify `src/cli/createProgram.ts` imports:

```ts
import { contextAffordanceWarnings } from "../kernel/affordanceSignals.js";
import {
  formatCandidateBlockText,
  formatSelectedMetadataText,
  formatTraceSectionText,
} from "../formatters/textFormatters.js";
```

Add this command before `get`:

```ts
  program
    .command("inspect")
    .argument("<queryOrQualifiedName>")
    .option("--limit <n>", "candidate search limit", numberOption, 5)
    .action(async (queryOrQualifiedName: string, options) => {
      const isQualifiedName = queryOrQualifiedName.includes(".");
      const candidates = isQualifiedName
        ? []
        : await deps.client.search(queryOrQualifiedName, { limit: options.limit });
      const selectedName = isQualifiedName ? queryOrQualifiedName : candidates[0]?.qualifiedName;

      if (!selectedName) {
        throw new Error(`inspect found no candidates for ${queryOrQualifiedName}`);
      }

      const selected = await deps.client.get(selectedName);
      const warnings = contextAffordanceWarnings(selected);
      const inbound = await deps.client.callers(selected.qualifiedName, { depth: 1 });
      const outbound = await deps.client.callees(selected.qualifiedName, { depth: 1 });

      writeLine(
        deps.writeOut,
        [
          candidates.length > 0 ? formatCandidateBlockText(candidates) : "",
          formatSelectedMetadataText(selected, warnings),
          formatTraceSectionText("inbound", inbound),
          formatTraceSectionText("outbound", outbound),
        ]
          .filter(Boolean)
          .join("\n\n"),
      );
    });
```

- [ ] **Step 4: Run CLI tests**

Run:

```bash
pnpm vitest run tests/cli.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit inspect command**

```bash
git add src/cli/createProgram.ts src/formatters/textFormatters.ts tests/cli.test.ts
git commit -m "feat: add inspect command"
```

## Task 9: Workflow Kit

**Files:**
- Create: `workflow/AGENTS.md`
- Create: `workflow/skills/codebase-memory/SKILL.md`
- Create: `workflow/skills/codebase-memory/agents/openai.yaml`
- Create: `workflow/skills/codebase-memory/agents/claude.md`
- Create: `src/workflows/createWorkflowFiles.ts`

- [ ] **Step 1: Create workflow source constants**

Create `src/workflows/createWorkflowFiles.ts`:

```ts
export const workflowAgentsMd = `# GCAL Workflow Rules

- Use \`gcal\` before raw text search for code symbols.
- Prefer graph and semantic lookup for classes, methods, callers, callees, and architecture.
- Use raw text search only for literals, configs, non-code files, or weak graph results.
- Search cheaply before fetching source.
- Inspect before source when unsure.
- Use \`gcal get\` only when exact source earns its place in context.
- Prefer exact qualified names when available.
- Treat full source as expensive.
- Do not include source only because a symbol appeared in search.
- Keep noisy tool responses out of conversation context.
- Do not implement or rely on \`gcal elect\` in Phase 1.
`;

export const codebaseMemorySkillMd = `---
name: codebase-memory
description: Use GCAL for code symbol discovery, context election, exact source retrieval, call tracing, architecture checks, and index status.
---

# Codebase Memory With GCAL

Use \`gcal\` for code context election before raw file reads or text search.

## Workflow

1. If you know a fully qualified name, use it.
2. Use \`gcal search\` or \`gcal symbol\` for cheap candidate discovery.
3. Use \`gcal inspect\` for metadata, signature, visibility, complexity, and graph hints.
4. Use \`gcal get\` only when exact source is likely needed.
5. Use \`gcal callers\` or \`gcal callees\` when graph relationships answer the task.
6. Use \`gcal arch\` for system shape and \`gcal status\` when index freshness is uncertain.

## Commands

\`\`\`bash
gcal search "BookingService" --limit 5
gcal symbol ".*BookingService.*" --limit 5
gcal inspect "com.example.BookingService.cancelBooking"
gcal get "com.example.BookingService.cancelBooking"
gcal callers "com.example.BookingService.cancelBooking" --depth 1
gcal callees "com.example.BookingService.cancelBooking" --depth 1
gcal arch
gcal status
\`\`\`

## Context Election Rules

- Do not include source because a symbol appeared in search.
- Prefer metadata over source when metadata answers the question.
- Prefer exact qualified names over broad text.
- Use raw text search for literals, configs, and non-code files.
- Keep tool output compact in the conversation.
`;
```

- [ ] **Step 2: Create workflow files from the same content**

Create `workflow/AGENTS.md` with the exact `workflowAgentsMd` content excluding the TypeScript backticks.

Create `workflow/skills/codebase-memory/SKILL.md` with the exact `codebaseMemorySkillMd` markdown content.

Create `workflow/skills/codebase-memory/agents/openai.yaml`:

```yaml
interface:
  display_name: "Codebase Memory"
  short_description: "Use GCAL for code context election"
  default_prompt: "Use $codebase-memory to search, inspect candidates, trace relationships, or get exact source with gcal."
```

Create `workflow/skills/codebase-memory/agents/claude.md`:

```md
# Codebase Memory

Use the `codebase-memory` skill when discovering code symbols, inspecting graph relationships, or deciding whether exact source belongs in context.
```

- [ ] **Step 3: Commit workflow kit**

```bash
git add src/workflows/createWorkflowFiles.ts workflow/AGENTS.md workflow/skills/codebase-memory/SKILL.md workflow/skills/codebase-memory/agents/openai.yaml workflow/skills/codebase-memory/agents/claude.md
git commit -m "feat: add GCAL workflow kit"
```

## Task 10: README And Final Verification

**Files:**
- Create: `README.md`
- Modify: `AGENTS.md` if needed to keep project instructions current.

- [ ] **Step 1: Create README**

Create `README.md`:

```md
# Goldeneye Code Agent Layer

Goldeneye Code Agent Layer (GCAL) is a local context-election CLI and workflow kit for people who use coding agents through Codex, Claude, or similar tools.

GCAL Phase 1 wraps `codebase-memory-mcp` with compact deterministic commands. It helps agents search, inspect, trace, and fetch exact source without flooding the session context.

## Install

```bash
pnpm install
pnpm build
```

## Configuration

```bash
export GCAL_MCP_URL=http://localhost:8767/mcp
export GCAL_PROJECT=my-indexed-project
```

`GCAL_MCP_URL` points to a gateway-compatible MCP HTTP endpoint. `GCAL_PROJECT` is the indexed `codebase-memory-mcp` project name.

## Commands

```bash
gcal search "BookingService" --limit 5
gcal symbol ".*BookingService.*" --limit 5
gcal inspect "com.example.BookingService.cancelBooking"
gcal get "com.example.BookingService.cancelBooking"
gcal callers "com.example.BookingService.cancelBooking" --depth 1
gcal callees "com.example.BookingService.cancelBooking" --depth 1
gcal arch
gcal status
gcal index .
```

## Phase 1 Boundary

GCAL does not implement `gcal elect` yet. Agents compose deterministic primitives first: search, inspect, get, trace, architecture, status, and index.

## Workflow Kit

Reusable agent instructions live in `workflow/`.
```

- [ ] **Step 2: Run complete verification**

Run:

```bash
pnpm check
```

Expected: lint, tests, and build all pass.

- [ ] **Step 3: Verify command help**

Run:

```bash
pnpm build
node dist/main.js --help
```

Expected: help output lists GCAL commands. If `GCAL_PROJECT` is required before help renders, move the `GCAL_PROJECT` check in `src/main.ts` after Commander parses help.

- [ ] **Step 4: Check git status**

Run:

```bash
git status --short
```

Expected: only intentional files are modified or untracked.

- [ ] **Step 5: Commit docs and final fixes**

```bash
git add README.md AGENTS.md dev-guidelines.md
git commit -m "docs: add project usage and agent guidance"
```

- [ ] **Step 6: Final implementation commit if needed**

If previous tasks left verified source changes uncommitted, commit them:

```bash
git add src tests workflow package.json pnpm-lock.yaml tsconfig.json vitest.config.ts eslint.config.js .prettierrc.json .gitignore README.md
git commit -m "feat: complete GCAL phase 1 primitives"
```

Expected: either a commit is created for remaining verified changes or Git reports that there is nothing to commit.

## Self-Review

Spec coverage:

- Phase 1 deterministic commands: Tasks 6, 7, and 8.
- No `gcal elect`: Scope and command list exclude it.
- Adapter boundary: Task 6.
- Response normalization: Task 3.
- Compact output contracts: Task 4.
- Trace hints and context-safety policies: Task 5.
- Workflow kit: Task 9.
- Fixture-based tests: Tasks 2 through 8.
- README and setup documentation: Task 10.

Placeholder scan:

- The plan contains no unresolved placeholder markers or unspecified implementation steps.
- Every code-writing step includes concrete file content or a concrete code block.

Type consistency:

- `CodebaseMemoryClient`, formatter functions, policy functions, and CLI tests use the same domain types from `src/domain/types.ts`.
- Command names match the approved Phase 1 command list.
