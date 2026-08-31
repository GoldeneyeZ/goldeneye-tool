import { describe, expect, it, vi } from "vitest";
import type { GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import type { SelectedSymbol, SymbolCandidate, TraceEdge } from "../src/domain/types.js";
import {
  formatJavaScriptWorkflowValue,
  MAX_JS_WORKFLOW_OUTPUT_BYTES,
  runJavaScriptWorkflow,
} from "../src/workflows/runJavaScriptWorkflow.js";

function candidate(qualifiedName: string): SymbolCandidate {
  return {
    qualifiedName,
    label: "Method",
    filePath: "src/Service.ts",
    line: 1,
    signature: "run(): void",
  };
}

function selected(qualifiedName: string, source: string): SelectedSymbol {
  return {
    qualifiedName,
    kind: "Method",
    filePath: "src/Service.ts",
    startLine: 1,
    endLine: 1,
    lines: 1,
    complexity: null,
    cognitive: null,
    visibility: "public",
    signature: "run(): void",
    returnType: "void",
    decorators: "",
    callers: null,
    callees: null,
    source,
  };
}

function edge(relatedQualifiedName: string): TraceEdge {
  return {
    sourceQualifiedName: relatedQualifiedName,
    targetQualifiedName: "example.Target.run",
    relatedQualifiedName,
    hop: 1,
    filePath: "src/Caller.ts",
    line: 2,
  };
}

function client(overrides: Partial<GcalBackendClient>): GcalBackendClient {
  const unused = vi.fn(async () => {
    throw new Error("unexpected backend call");
  });
  return {
    search: unused,
    symbol: unused,
    get: unused,
    callers: unused,
    callees: unused,
    arch: unused,
    status: unused,
    index: unused,
    projects: unused,
    ...overrides,
  };
}

describe("runJavaScriptWorkflow", () => {
  it("runs adaptive JavaScript loops over search, source, and relationship results", async () => {
    const hits = [candidate("example.First.run"), candidate("example.TokenService.run")];
    const search = vi.fn().mockResolvedValue(hits);
    const get = vi.fn(async (qualifiedName: string) =>
      selected(qualifiedName, qualifiedName.includes("Token") ? "validate token" : "plain source"),
    );
    const callers = vi.fn().mockResolvedValue([edge("example.Controller.run")]);
    const result = await runJavaScriptWorkflow(
      client({ search, get, callers }),
      `
        const matches = await gcal.search("authentication", { limit: 5 });
        const chosen = [];
        for (const match of matches) {
          const source = await gcal.source(match.qualifiedName);
          if (source.source.includes("token")) {
            chosen.push({
              qualifiedName: match.qualifiedName,
              callers: await gcal.callers(match.qualifiedName, { depth: 2, limit: 10 }),
            });
          }
        }
        return chosen;
      `,
      { maxCalls: 10, timeoutMs: 2_000 },
    );

    expect(result.callCount).toBe(4);
    expect(search).toHaveBeenCalledWith("authentication", {
      limit: 5,
      label: undefined,
      filePattern: undefined,
      qualifiedNamePattern: undefined,
    });
    expect(get).toHaveBeenCalledTimes(2);
    expect(callers).toHaveBeenCalledWith("example.TokenService.run", { depth: 2 });
    expect(result.value).toEqual([
      {
        qualifiedName: "example.TokenService.run",
        callers: [edge("example.Controller.run")],
      },
    ]);
  });

  it("supports parallel JavaScript branches and captures console output", async () => {
    const callers = vi.fn().mockResolvedValue([edge("inbound")]);
    const callees = vi.fn().mockResolvedValue([edge("outbound")]);
    const result = await runJavaScriptWorkflow(
      client({ callers, callees }),
      `
        console.log("tracing target");
        const [inbound, outbound] = await Promise.all([
          gcal.callers("example.Target.run"),
          gcal.callees("example.Target.run"),
        ]);
        return { inbound, outbound };
      `,
      { maxCalls: 2, timeoutMs: 2_000 },
    );

    expect(result.callCount).toBe(2);
    expect(result.stdout).toBe("tracing target\n");
    expect(result.value).toEqual({ inbound: [edge("inbound")], outbound: [edge("outbound")] });
  });

  it("can settle source failures without aborting the workflow", async () => {
    const get = vi.fn(async (qualifiedName: string) => {
      if (qualifiedName === "example.Project") throw new Error("symbol has no indexed file");
      return selected(qualifiedName, "source");
    });
    const result = await runJavaScriptWorkflow(
      client({ get }),
      `return await Promise.all([
        gcal.trySource("example.Project"),
        gcal.trySource("example.Service.run"),
      ]);`,
      { maxCalls: 2, timeoutMs: 2_000 },
    );

    expect(result.callCount).toBe(2);
    expect(result.value).toEqual([
      { ok: false, error: "symbol has no indexed file" },
      { ok: true, ...selected("example.Service.run", "source") },
    ]);
  });

  it("can settle optional trace failures without aborting the workflow", async () => {
    const callers = vi.fn().mockRejectedValue(new Error("symbol was not found"));
    const callees = vi.fn().mockResolvedValue([edge("outbound")]);
    const result = await runJavaScriptWorkflow(
      client({ callers, callees }),
      `return await Promise.all([
        gcal.tryCallers("example.Missing.run"),
        gcal.tryCallees("example.Target.run"),
      ]);`,
      { maxCalls: 2, timeoutMs: 2_000 },
    );

    expect(result.callCount).toBe(2);
    expect(result.value).toEqual([
      { ok: false, error: "symbol was not found" },
      { ok: true, edges: [edge("outbound")] },
    ]);
  });

  it("terminates scripts exceeding backend-call or wall-clock budgets", async () => {
    const search = vi.fn().mockResolvedValue([]);

    await expect(
      runJavaScriptWorkflow(
        client({ search }),
        `await gcal.search("one"); await gcal.search("two"); return [];`,
        { maxCalls: 1, timeoutMs: 2_000 },
      ),
    ).rejects.toThrow("exceeded 1 backend calls");
    await expect(
      runJavaScriptWorkflow(client({}), `while (true) {}`, {
        maxCalls: 1,
        timeoutMs: 50,
      }),
    ).rejects.toThrow("timed out after 50 ms");
    await expect(
      runJavaScriptWorkflow(client({}), `process.exit(0);`, {
        maxCalls: 1,
        timeoutMs: 2_000,
      }),
    ).rejects.toThrow("worker exited before returning (code 0)");
  });

  it("truncates console output on a valid UTF-8 boundary", async () => {
    const result = await runJavaScriptWorkflow(
      client({}),
      `console.log("🟡".repeat(3000)); return true;`,
      { maxCalls: 1, timeoutMs: 2_000 },
    );

    expect(result.logsTruncated).toBe(true);
    expect(result.stdout).not.toContain("�");
    expect(Buffer.byteLength(result.stdout, "utf8")).toBeLessThanOrEqual(8 * 1024);
  });

  it("requires bounded serializable output", () => {
    expect(formatJavaScriptWorkflowValue({ ok: true })).toBe('{"ok":true}');
    expect(() => formatJavaScriptWorkflowValue(undefined)).toThrow("serializable value");
    const cyclic: { self?: unknown } = {};
    cyclic.self = cyclic;
    expect(() => formatJavaScriptWorkflowValue(cyclic)).toThrow("serializable value");
    expect(() =>
      formatJavaScriptWorkflowValue("x".repeat(MAX_JS_WORKFLOW_OUTPUT_BYTES + 1)),
    ).toThrow("output exceeds");
  });
});
