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
