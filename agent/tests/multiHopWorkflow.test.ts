import { describe, expect, it, vi } from "vitest";
import type { GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import type { SelectedSymbol, SymbolCandidate, TraceEdge } from "../src/domain/types.js";
import {
  runMultiHopWorkflow,
  WORKFLOW_SOURCE_CHUNK_BYTES,
} from "../src/workflows/runMultiHopWorkflow.js";

function candidate(qualifiedName: string): SymbolCandidate {
  return {
    qualifiedName,
    label: "Method",
    filePath: "src/Service.ts",
    line: 10,
    signature: "run(): void",
  };
}

function selected(qualifiedName: string): SelectedSymbol {
  return {
    qualifiedName,
    kind: "Method",
    filePath: "src/Service.ts",
    startLine: 10,
    endLine: 12,
    lines: 3,
    complexity: 1,
    cognitive: 0,
    visibility: "public",
    signature: "run(): void",
    returnType: "void",
    decorators: "",
    callers: 1,
    callees: 1,
    source: "run(): void {}",
  };
}

function edge(relatedQualifiedName: string): TraceEdge {
  return {
    sourceQualifiedName: "example.Caller.run",
    targetQualifiedName: "example.Service.run",
    relatedQualifiedName,
    hop: 1,
    filePath: "src/Service.ts",
    line: 10,
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

const baseOptions = {
  exact: false,
  rank: 1,
  searchLimit: 5,
  source: false,
  callers: false,
  callees: false,
  depth: 1,
  traceLimit: 20,
};

describe("runMultiHopWorkflow", () => {
  it("feeds a selected search rank into one bounded source hop", async () => {
    const candidates = [candidate("example.First.run"), candidate("example.Second.run")];
    const search = vi.fn().mockResolvedValue(candidates);
    const getSnippetChunk = vi.fn().mockResolvedValue(selected(candidates[1].qualifiedName));

    const result = await runMultiHopWorkflow(client({ search, getSnippetChunk }), "service run", {
      ...baseOptions,
      rank: 2,
      source: true,
    });

    expect(search).toHaveBeenCalledWith("service run", { limit: 5 });
    expect(getSnippetChunk).toHaveBeenCalledWith(candidates[1].qualifiedName, {
      chunk: 1,
      chunkBytes: WORKFLOW_SOURCE_CHUNK_BYTES,
    });
    expect(result.selectedQualifiedName).toBe(candidates[1].qualifiedName);
    expect(result.source?.source).toBe("run(): void {}");
  });

  it("starts independent source and relationship hops concurrently", async () => {
    let resolveSource!: (value: SelectedSymbol) => void;
    let resolveCallers!: (value: TraceEdge[]) => void;
    let resolveCallees!: (value: TraceEdge[]) => void;
    const get = vi.fn(() => new Promise<SelectedSymbol>((resolve) => (resolveSource = resolve)));
    const callers = vi.fn(() => new Promise<TraceEdge[]>((resolve) => (resolveCallers = resolve)));
    const callees = vi.fn(() => new Promise<TraceEdge[]>((resolve) => (resolveCallees = resolve)));
    const workflow = runMultiHopWorkflow(client({ get, callers, callees }), "example.Service.run", {
      ...baseOptions,
      exact: true,
      source: true,
      callers: true,
      callees: true,
    });

    await vi.waitFor(() => {
      expect(get).toHaveBeenCalledOnce();
      expect(callers).toHaveBeenCalledOnce();
      expect(callees).toHaveBeenCalledOnce();
    });
    resolveSource(selected("example.Service.run"));
    resolveCallers([edge("example.Caller.run")]);
    resolveCallees([edge("example.Dependency.run")]);

    const result = await workflow;
    expect(result.failures).toEqual([]);
    expect(result.inbound?.[0].relatedQualifiedName).toBe("example.Caller.run");
    expect(result.outbound?.[0].relatedQualifiedName).toBe("example.Dependency.run");
  });

  it("preserves successful hops, reports failures deterministically, and bounds traces", async () => {
    const callers = vi.fn().mockRejectedValue(new Error("backend\nfailed"));
    const callees = vi.fn().mockResolvedValue([edge("one"), edge("two"), edge("three")]);

    const result = await runMultiHopWorkflow(client({ callers, callees }), "example.Service.run", {
      ...baseOptions,
      exact: true,
      callers: true,
      callees: true,
      traceLimit: 2,
    });

    expect(result.failures).toEqual([{ hop: "callers", message: "backend failed" }]);
    expect(result.inbound).toBeUndefined();
    expect(result.outbound).toHaveLength(2);
    expect(result.outboundTotal).toBe(3);
  });

  it("rejects empty workflows and unsafe fan-out bounds before backend calls", async () => {
    const search = vi.fn();
    const backend = client({ search });

    await expect(runMultiHopWorkflow(backend, "service", baseOptions)).rejects.toThrow(
      "requires --source, --callers, --callees, or --all",
    );
    await expect(
      runMultiHopWorkflow(backend, "service", {
        ...baseOptions,
        callers: true,
        depth: 5,
      }),
    ).rejects.toThrow("--depth must be between 1 and 4");
    expect(search).not.toHaveBeenCalled();
  });
});
