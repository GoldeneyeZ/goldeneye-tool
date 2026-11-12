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
