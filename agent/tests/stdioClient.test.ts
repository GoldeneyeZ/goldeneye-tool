import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { describe, expect, it, vi } from "vitest";
import { StdioCodebaseMemoryClient } from "../src/adapters/codebaseMemoryMcp/StdioCodebaseMemoryClient.js";
import { StdioGoldeneyeClient } from "../src/adapters/goldeneye/StdioGoldeneyeClient.js";
import { architectureResponse } from "./fixtures/codebaseMemory.js";

class FakeChildProcess extends EventEmitter {
  readonly stdin = new PassThrough();
  readonly stdout = new PassThrough();
  readonly stderr = new PassThrough();
  readonly kill = vi.fn((signal?: NodeJS.Signals | number) => {
    this.emit("close", 0, signal ?? null);
    return true;
  });

  constructor(closeOnStdinEnd = true) {
    super();
    if (closeOnStdinEnd) {
      this.stdin.once("finish", () => queueMicrotask(() => this.emit("close", 0, null)));
    }
  }
}

function createClient(
  child: FakeChildProcess,
  onRequest: (request: Record<string, unknown>) => void,
): StdioCodebaseMemoryClient {
  let buffered = "";
  child.stdin.on("data", (chunk: Buffer) => {
    buffered += chunk.toString();
    const lines = buffered.split("\n");
    buffered = lines.pop() ?? "";

    for (const line of lines) {
      if (line.length > 0) {
        onRequest(JSON.parse(line) as Record<string, unknown>);
      }
    }
  });

  return new StdioCodebaseMemoryClient({
    command: "codebase-memory-mcp",
    project: "example-project",
    spawn: vi.fn(() => child) as never,
  });
}

function respond(child: FakeChildProcess, response: Record<string, unknown>): void {
  queueMicrotask(() => child.stdout.write(`${JSON.stringify(response)}\n`));
}

describe("StdioCodebaseMemoryClient", () => {
  it("initializes once then invokes index_status directly", async () => {
    const child = new FakeChildProcess();
    const requests: Array<Record<string, unknown>> = [];
    const client = createClient(child, (request) => {
      requests.push(request);

      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        respond(child, {
          jsonrpc: "2.0",
          id: request.id,
          result: { content: [{ type: "text", text: '{"status":"ready"}' }] },
        });
      }
    });

    await expect(client.status()).resolves.toEqual({ status: "ready" });

    expect(requests).toEqual([
      expect.objectContaining({
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: expect.objectContaining({ protocolVersion: "2024-11-05" }),
      }),
      { jsonrpc: "2.0", method: "notifications/initialized" },
      {
        jsonrpc: "2.0",
        id: 2,
        method: "tools/call",
        params: {
          name: "index_status",
          arguments: { project: "example-project" },
        },
      },
    ]);

    await client.close();
    expect(child.stdin.writableEnded).toBe(true);
    expect(child.kill).not.toHaveBeenCalled();
  });

  it("force-kills the MCP process when graceful stdin close times out", async () => {
    vi.useFakeTimers();
    try {
      const child = new FakeChildProcess(false);
      const client = createClient(child, (request) => {
        if (request.method === "initialize") {
          respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
        }

        if (request.method === "tools/call") {
          respond(child, {
            jsonrpc: "2.0",
            id: request.id,
            result: { content: [{ type: "text", text: '{"status":"ready"}' }] },
          });
        }
      });

      await expect(client.status()).resolves.toEqual({ status: "ready" });
      const closing = client.close();
      await vi.advanceTimersByTimeAsync(5_000);
      await closing;

      expect(child.stdin.writableEnded).toBe(true);
      expect(child.kill).toHaveBeenCalledOnce();
    } finally {
      vi.useRealTimers();
    }
  });

  it("allows a slow graceful MCP shutdown to finish without force-killing", async () => {
    vi.useFakeTimers();
    try {
      const child = new FakeChildProcess(false);
      const client = createClient(child, (request) => {
        if (request.method === "initialize") {
          respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
        }

        if (request.method === "tools/call") {
          respond(child, {
            jsonrpc: "2.0",
            id: request.id,
            result: { content: [{ type: "text", text: '{"status":"ready"}' }] },
          });
        }
      });

      await expect(client.status()).resolves.toEqual({ status: "ready" });
      const closing = client.close();
      setTimeout(() => child.emit("close", 0, null), 2_500);
      await vi.advanceTimersByTimeAsync(2_500);
      await closing;

      expect(child.stdin.writableEnded).toBe(true);
      expect(child.kill).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("unwraps direct MCP JSON content before architecture normalization", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        respond(child, {
          jsonrpc: "2.0",
          id: request.id,
          result: { content: [{ type: "text", text: JSON.stringify(architectureResponse) }] },
        });
      }
    });

    await expect(client.arch()).resolves.toMatchObject({
      project: "example-project",
      total_nodes: 1000,
      languages: architectureResponse.languages,
    });

    await client.close();
  });

  it("correlates direct tool responses by numeric request ID", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        respond(child, { jsonrpc: "2.0", id: 99, result: { ignored: true } });
        respond(child, { jsonrpc: "2.0", id: request.id, result: { status: "ready" } });
      }
    });

    await expect(client.status()).resolves.toEqual({ status: "ready" });

    await client.close();
  });

  it("rejects JSON-RPC errors with concise MCP text", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        respond(child, {
          jsonrpc: "2.0",
          id: request.id,
          error: { code: -32603, message: "index unavailable" },
        });
      }
    });

    await expect(client.status()).rejects.toThrow("MCP error: index unavailable");

    await client.close();
  });

  it("rejects direct MCP tool errors from content payloads", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        respond(child, {
          jsonrpc: "2.0",
          id: request.id,
          result: { isError: true, content: [{ type: "text", text: "index unavailable" }] },
        });
      }
    });

    await expect(client.status()).rejects.toThrow("MCP tool error: index unavailable");

    await client.close();
  });

  it("ignores malformed stdout lines before a valid response", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        child.stdout.write("not json\n");
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        child.stdout.write("[]\n");
        respond(child, { jsonrpc: "2.0", id: request.id, result: { status: "ready" } });
      }
    });

    await expect(client.status()).resolves.toEqual({ status: "ready" });

    await client.close();
  });

  it("rejects a malformed response for an active request", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        respond(child, { jsonrpc: "2.0", id: request.id });
      }
    });

    await expect(client.status()).rejects.toThrow("MCP response did not include a result");

    await client.close();
  });

  it("rejects a pending request when child emits an error", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        queueMicrotask(() => child.emit("error", new Error("startup failed")));
      }
    });

    await expect(client.status()).rejects.toThrow("MCP process error: startup failed");
  });

  it("rejects a pending request when child exits before response", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        queueMicrotask(() => {
          child.stderr.write("backend exploded");
          child.emit("exit", 1, null);
        });
      }
    });

    await expect(client.status()).rejects.toThrow(
      "MCP process exited before response (code=1 signal=null): backend exploded",
    );
  });

  it("rejects a pending request when child closes before response", async () => {
    const child = new FakeChildProcess();
    const client = createClient(child, (request) => {
      if (request.method === "initialize") {
        respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
      }

      if (request.method === "tools/call") {
        queueMicrotask(() => child.emit("close", 1, null));
      }
    });

    await expect(client.status()).rejects.toThrow(
      "MCP process closed before response (code=1 signal=null)",
    );
  });
});

describe("StdioGoldeneyeClient", () => {
  it("disables watcher mode and invokes one direct snippet chunk call", async () => {
    const previous = process.env.GOLDENEYE_WATCHER_ENABLED;
    process.env.GOLDENEYE_WATCHER_ENABLED = "0";
    const child = new FakeChildProcess();
    const requests: Array<Record<string, unknown>> = [];
    let buffered = "";
    child.stdin.on("data", (chunk: Buffer) => {
      buffered += chunk.toString();
      const lines = buffered.split("\n");
      buffered = lines.pop() ?? "";

      for (const line of lines) {
        if (line.length === 0) continue;
        const request = JSON.parse(line) as Record<string, unknown>;
        requests.push(request);
        if (request.method === "initialize") {
          respond(child, { jsonrpc: "2.0", id: request.id, result: { capabilities: {} } });
        }
        if (request.method === "tools/call") {
          respond(child, {
            jsonrpc: "2.0",
            id: request.id,
            result: {
              content: [
                {
                  type: "text",
                  text: JSON.stringify({
                    qualified_name: "example.huge",
                    label: "Function",
                    file_path: "src/example.ts",
                    start_line: 1,
                    end_line: 100,
                    source: "λ source",
                    source_bytes: 8,
                    source_lines: 1,
                    source_sha256: "a".repeat(64),
                    indexed_file_hash: "indexed",
                    chunk_bytes: 8192,
                    chunk_count: 1,
                    chunk: 1,
                    chunk_start_byte: 0,
                    chunk_end_byte: 8,
                    eof: true,
                    truncated: false,
                  }),
                },
              ],
            },
          });
        }
      }
    });
    const spawnMock = vi.fn(() => child);
    const client = new StdioGoldeneyeClient({
      command: "goldeneye",
      project: "example-project",
      spawn: spawnMock as never,
    });

    try {
      const selected = await client.getSnippetChunk("example.huge", {
        chunk: 1,
        chunkBytes: 8192,
        expectedSourceSha256: "a".repeat(64),
      });

      const spawnOptions = spawnMock.mock.calls[0]?.[2] as
        | { env?: NodeJS.ProcessEnv }
        | undefined;
      expect(spawnOptions?.env).toMatchObject({
        GOLDENEYE_WATCHER_ENABLED: "0",
      });
      expect(
        requests.filter(
          (request) =>
            request.method === "tools/call" &&
            (request.params as { name?: string } | undefined)?.name ===
              "get_code_snippet_chunk",
        ),
      ).toHaveLength(1);
      expect(selected.source).toBe("λ source");
      expect(selected.sourceChunk).toMatchObject({
        chunk: 1,
        chunkBytes: 8192,
        chunkCount: 1,
        sourceSha256: "a".repeat(64),
      });
    } finally {
      await client.close();
      if (previous === undefined) {
        delete process.env.GOLDENEYE_WATCHER_ENABLED;
      } else {
        process.env.GOLDENEYE_WATCHER_ENABLED = previous;
      }
    }
  });
});
