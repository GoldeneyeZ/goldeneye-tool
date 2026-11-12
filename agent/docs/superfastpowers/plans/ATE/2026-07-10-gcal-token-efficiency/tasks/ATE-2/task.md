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
