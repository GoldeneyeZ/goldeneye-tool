import { describe, expect, it, vi } from "vitest";
import { CodebaseMemoryMcpClient } from "../src/adapters/codebaseMemoryMcp/CodebaseMemoryMcpClient.js";
import { GatewayCodebaseMemoryClient } from "../src/adapters/codebaseMemoryMcp/GatewayCodebaseMemoryClient.js";
import { GcalBackendError } from "../src/domain/GcalBackendClient.js";
import {
  architectureResponse,
  inboundTraceResponse,
  legacyInboundTraceResponse,
  methodSnippetResponse,
  outboundTraceResponse,
  searchGraphResponse,
} from "./fixtures/codebaseMemory.js";

function jsonResponse(body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function requestBody(fetchMock: ReturnType<typeof vi.fn<typeof fetch>>): Record<string, unknown> {
  const init = fetchMock.mock.calls[0]?.[1];
  const rawBody = init && "body" in init ? init.body : undefined;
  return JSON.parse(String(rawBody)) as Record<string, unknown>;
}

describe("GatewayCodebaseMemoryClient", () => {
  it("lists and normalizes indexed projects through the shared client", async () => {
    const invoke = vi.fn(async () => ({
      projects: [{ name: "example-project", root_path: "C:/code/example" }],
    }));
    const client = new CodebaseMemoryMcpClient("", { invoke });

    await expect(client.projects()).resolves.toEqual([
      { name: "example-project", rootPath: "C:/code/example" },
    ]);
    expect(invoke).toHaveBeenCalledWith("list_projects", {});
  });

  it("uses the direct index_status tool name through the shared client", async () => {
    const invoke = vi.fn(async () => ({ status: "ready" }));
    const client = new CodebaseMemoryMcpClient("example-project", { invoke });

    await client.status();

    expect(invoke).toHaveBeenCalledWith("index_status", { project: "example-project" });
  });

  it("normalizes manifest/chunks and preserves UTF-8 reconstruction with exact tool args", async () => {
    const sha = "c".repeat(64);
    const base = {
      qualified_name: "example.large",
      label: "Function",
      file_path: "src/example.ts",
      start_line: 1,
      end_line: 2,
      source_bytes: 6,
      source_lines: 2,
      source_sha256: sha,
      indexed_file_hash: "indexed",
      chunk_bytes: 256,
      chunk_count: 2,
    };
    const invoke = vi.fn(async (toolName: string, args: Record<string, unknown>) => {
      if (toolName === "get_code_snippet_manifest") return base;
      const chunk = args.chunk as number;
      return {
        ...base,
        source: chunk === 1 ? "λA" : "βB",
        chunk,
        chunk_start_byte: chunk === 1 ? 0 : 3,
        chunk_end_byte: chunk === 1 ? 3 : 6,
        eof: chunk === 2,
        truncated: chunk === 1,
      };
    });
    const client = new CodebaseMemoryMcpClient("example-project", { invoke });

    const manifest = await client.getSnippetManifest("example.large", 256);
    const first = await client.getSnippetChunk("example.large", {
      chunk: 1,
      chunkBytes: 256,
      expectedSourceSha256: sha,
    });
    const second = await client.getSnippetChunk("example.large", {
      chunk: 2,
      chunkBytes: 256,
      expectedSourceSha256: sha,
    });

    expect(manifest).toMatchObject({ chunkBytes: 256, chunkCount: 2, sourceSha256: sha });
    expect(first.source + second.source).toBe("λAβB");
    expect(first.sourceChunk).toMatchObject({ chunk: 1, chunkStartByte: 0, chunkEndByte: 3 });
    expect(second.sourceChunk).toMatchObject({ chunk: 2, eof: true });
    expect(invoke.mock.calls).toEqual([
      [
        "get_code_snippet_manifest",
        { project: "example-project", qualified_name: "example.large", chunk_bytes: 256 },
      ],
      [
        "get_code_snippet_chunk",
        {
          project: "example-project",
          qualified_name: "example.large",
          chunk: 1,
          chunk_bytes: 256,
          expected_source_sha256: sha,
        },
      ],
      [
        "get_code_snippet_chunk",
        {
          project: "example-project",
          qualified_name: "example.large",
          chunk: 2,
          chunk_bytes: 256,
          expected_source_sha256: sha,
        },
      ],
    ]);
  });

  it("requests high-signal architecture aspects and returns a bounded projection", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [{ text: JSON.stringify(architectureResponse) }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.arch();

    expect(result).toEqual({
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
    expect(requestBody(fetchMock)).toMatchObject({
      params: {
        arguments: {
          id: "codebase-memory-mcp::get_architecture",
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
          },
        },
      },
    });
  });

  it("invokes codebase-memory search_graph through gateway.invoke and normalizes the response", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [{ text: JSON.stringify(searchGraphResponse) }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.search("BookingService", { limit: 5 });

    expect(result).toEqual([
      {
        qualifiedName: "com.example.booking.BookingService.cancelBooking",
        label: "Method",
        filePath: "src/main/java/com/example/booking/BookingService.java",
        line: 42,
        signature: "public BookingResponse cancelBooking(String bookingId)",
      },
    ]);
    expect(fetchMock).toHaveBeenCalledOnce();
    expect(requestBody(fetchMock)).toMatchObject({
      method: "tools/call",
      params: {
        name: "gateway.invoke",
        arguments: {
          id: "codebase-memory-mcp::search_graph",
          args: {
            project: "example-project",
            query: '"BookingService"',
            limit: 5,
          },
        },
      },
    });
  });

  it("invokes index_status through gateway.invoke with the configured project", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [{ text: JSON.stringify({ status: "ready" }) }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    await client.status();

    expect(requestBody(fetchMock)).toMatchObject({
      params: {
        name: "gateway.invoke",
        arguments: {
          id: "codebase-memory-mcp::index_status",
          args: { project: "example-project" },
        },
      },
    });
  });

  it("invokes get_code_snippet through gateway.invoke and normalizes already parsed content", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [{ text: methodSnippetResponse }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.get("com.example.booking.BookingService.cancelBooking");

    expect(result).toMatchObject({
      qualifiedName: "com.example.booking.BookingService.cancelBooking",
      kind: "Method",
      source: methodSnippetResponse.code,
    });
    expect(requestBody(fetchMock)).toMatchObject({
      params: {
        arguments: {
          id: "codebase-memory-mcp::get_code_snippet",
          args: {
            project: "example-project",
            qualified_name: "com.example.booking.BookingService.cancelBooking",
          },
        },
      },
    });
  });

  it("maps caller traces into explicit trace edges", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [{ text: JSON.stringify(inboundTraceResponse) }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.callers("com.example.booking.BookingService.cancelBooking", {
      depth: 1,
    });

    expect(result).toEqual([
      {
        sourceQualifiedName: "com.example.booking.BookingController.cancelBooking",
        targetQualifiedName: "com.example.booking.BookingService.cancelBooking",
        relatedQualifiedName: "com.example.booking.BookingController.cancelBooking",
        hop: 1,
        filePath: "src/main/java/com/example/booking/BookingController.java",
        line: 31,
      },
    ]);
    expect(requestBody(fetchMock)).toMatchObject({
      params: {
        arguments: {
          id: "codebase-memory-mcp::trace_path",
          args: {
            project: "example-project",
            function_name: "com.example.booking.BookingService.cancelBooking",
            direction: "inbound",
            depth: 1,
            mode: "calls",
          },
        },
      },
    });
  });

  it("maps callee traces from the live MCP response into explicit trace edges", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [{ text: JSON.stringify(outboundTraceResponse) }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.callees("com.example.booking.BookingService.cancelBooking", {
      depth: 1,
    });

    expect(result).toEqual([
      {
        sourceQualifiedName: "com.example.booking.BookingService.cancelBooking",
        targetQualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
        relatedQualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
        hop: 1,
        filePath: "src/main/java/com/example/booking/BookingRepository.java",
        line: 73,
      },
    ]);
  });

  it("retains compatibility with legacy path trace responses", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [{ text: JSON.stringify(legacyInboundTraceResponse) }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.callers("com.example.booking.BookingService.cancelBooking", {
      depth: 1,
    });

    expect(result).toEqual([
      {
        sourceQualifiedName: "com.example.booking.BookingController.cancelBooking",
        targetQualifiedName: "com.example.booking.BookingService.cancelBooking",
        relatedQualifiedName: "com.example.booking.BookingController.cancelBooking",
        hop: null,
        filePath: "src/main/java/com/example/booking/BookingController.java",
        line: 31,
      },
    ]);
  });

  it("throws concise errors for JSON-RPC gateway failures", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        error: {
          code: -32603,
          message: "gateway.invoke failed",
          data: {
            stack: "irrelevant verbose stack",
          },
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    await expect(client.status()).rejects.toThrow("MCP error: gateway.invoke failed");
  });

  it("throws concise errors for MCP tool-result failures", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          isError: true,
          content: [{ text: "tool exploded" }],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    await expect(client.status()).rejects.toThrow("MCP tool error: tool exploded");
  });

  it("preserves typed MCP tool errors without changing their concise message", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          isError: true,
          content: [{ text: "snippet exceeds bounds: 900 lines" }],
          structuredContent: {
            code: "SnippetTooLarge",
            message: "snippet exceeds bounds: 900 lines",
            details: { actual_lines: 900 },
          },
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const error = await client.get("example.huge").catch((caught: unknown) => caught);

    expect(error).toBeInstanceOf(GcalBackendError);
    expect(error).toMatchObject({
      message: "MCP tool error: snippet exceeds bounds: 900 lines",
      code: "SnippetTooLarge",
      details: { actual_lines: 900 },
    });
  });

  it("throws concise errors for nested MCP tool-result failures", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [
            {
              text: JSON.stringify({
                isError: true,
                content: [{ text: "inner boom" }],
              }),
            },
          ],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    await expect(client.status()).rejects.toThrow("MCP tool error: inner boom");
  });

  it("unwraps nested MCP content wrappers before normalizing search results", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      jsonResponse({
        result: {
          content: [
            {
              text: JSON.stringify({
                content: [{ text: JSON.stringify(searchGraphResponse) }],
              }),
            },
          ],
        },
      }),
    );
    const client = new GatewayCodebaseMemoryClient({
      mcpUrl: "http://localhost:8767/mcp",
      project: "example-project",
      fetch: fetchMock,
    });

    const result = await client.search("BookingService", { limit: 5 });

    expect(result).toEqual([
      {
        qualifiedName: "com.example.booking.BookingService.cancelBooking",
        label: "Method",
        filePath: "src/main/java/com/example/booking/BookingService.java",
        line: 42,
        signature: "public BookingResponse cancelBooking(String bookingId)",
      },
    ]);
  });
});
