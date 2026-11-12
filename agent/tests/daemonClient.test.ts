import { randomUUID } from "node:crypto";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GcalBackendError, type GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import { DaemonGcalBackendClient } from "../src/adapters/daemon/DaemonGcalBackendClient.js";
import {
  daemonEndpoint,
  daemonLockPath,
  startDetachedDaemon,
} from "../src/adapters/daemon/daemonEndpoint.js";
import { startDaemonServer } from "../src/daemon/startDaemonServer.js";

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories
      .splice(0)
      .map((directory) => rm(directory, { recursive: true, force: true })),
  );
});

describe("GCAL daemon client", () => {
  it("starts detached daemon outside the caller working directory", async () => {
    const gcalHome = await temporaryDirectory();
    const unref = vi.fn();
    const spawnProcess = vi.fn(() => ({ unref }) as never);

    startDetachedDaemon({
      gcalHome,
      endpoint: daemonEndpoint(gcalHome),
      idleTimeoutMs: 600_000,
      spawnProcess,
    });

    expect(spawnProcess).toHaveBeenCalledOnce();
    expect(spawnProcess.mock.calls[0]?.[2]).toMatchObject({
      cwd: tmpdir(),
      detached: true,
      stdio: "ignore",
    });
    expect(unref).toHaveBeenCalledOnce();
  });

  it("reuses one backend session and closes it after idle expiry", async () => {
    const gcalHome = await temporaryDirectory();
    const endpoint = daemonEndpoint(gcalHome);
    const close = vi.fn(async () => undefined);
    const status = vi
      .fn()
      .mockResolvedValueOnce({ generation: 1 })
      .mockResolvedValueOnce({ generation: 2 });
    const createClient = vi.fn(() => fakeBackend({ status, close }));
    const daemon = await startDaemonServer({
      endpoint,
      lockPath: daemonLockPath(gcalHome),
      idleTimeoutMs: 100,
      createClient,
    });
    const client = remoteClient(gcalHome, endpoint);

    expect(await client.status()).toEqual({ generation: 1 });
    expect(await client.status()).toEqual({ generation: 2 });
    expect(createClient).toHaveBeenCalledOnce();
    expect(createClient).toHaveBeenCalledWith("goldeneye", "project-a");

    await daemon.closed;
    expect(close).toHaveBeenCalledOnce();
  });

  it("preserves typed backend errors across IPC", async () => {
    const gcalHome = await temporaryDirectory();
    const endpoint = daemonEndpoint(gcalHome);
    const daemon = await startDaemonServer({
      endpoint,
      lockPath: daemonLockPath(gcalHome),
      idleTimeoutMs: 1_000,
      createClient: () =>
        fakeBackend({
          arch: vi
            .fn()
            .mockRejectedValue(new GcalBackendError("query failed", "SQLITE", { row: 4 })),
        }),
    });
    const client = remoteClient(gcalHome, endpoint);

    const error = await client.arch().catch((caught: unknown) => caught);
    expect(error).toBeInstanceOf(GcalBackendError);
    expect(error).toMatchObject({
      message: "query failed",
      code: "SQLITE",
      details: { row: 4 },
    });

    await daemon.close();
  });

  it("maps the complete backend contract across IPC", async () => {
    const gcalHome = await temporaryDirectory();
    const endpoint = daemonEndpoint(gcalHome);
    const selected = selectedSymbol("example.Get", "class Get {}");
    const backend = fakeBackend({
      search: vi.fn().mockResolvedValue([candidate("example.Search")]),
      symbol: vi.fn().mockResolvedValue([candidate("example.Symbol")]),
      get: vi.fn().mockResolvedValue(selected),
      getSnippetManifest: vi.fn().mockResolvedValue({
        selected,
        sourceSha256: "a".repeat(64),
        sourceBytes: 100,
        sourceLines: 4,
        indexedFileHash: "indexed",
        chunkBytes: 50,
        chunkCount: 2,
      }),
      getSnippetChunk: vi.fn().mockResolvedValue(selectedSymbol("example.Get", "class")),
      callers: vi.fn().mockResolvedValue([traceEdge("a", "b")]),
      callees: vi.fn().mockResolvedValue([traceEdge("b", "c")]),
      arch: vi.fn().mockResolvedValue({ modules: 2 }),
      status: vi.fn().mockResolvedValue({ ready: true }),
      index: vi.fn().mockResolvedValue({ indexed: true }),
      projects: vi.fn().mockResolvedValue([{ name: "project-a", rootPath: "/repo" }]),
    });
    const daemon = await startDaemonServer({
      endpoint,
      lockPath: daemonLockPath(gcalHome),
      idleTimeoutMs: 1_000,
      createClient: () => backend,
    });
    const client = remoteClient(gcalHome, endpoint);

    expect(await client.search("Search", { limit: 5 })).toHaveLength(1);
    expect(await client.symbol("Symbol", { limit: 5 })).toHaveLength(1);
    expect((await client.get("example.Get")).source).toContain("class");
    expect((await client.getSnippetManifest("example.Get", 50)).chunkCount).toBe(2);
    expect(
      (
        await client.getSnippetChunk("example.Get", {
          chunk: 1,
          chunkBytes: 50,
          expectedSourceSha256: "a".repeat(64),
        })
      ).source,
    ).toBe("class");
    expect(await client.callers("example.Get", { depth: 1 })).toHaveLength(1);
    expect(await client.callees("example.Get", { depth: 1 })).toHaveLength(1);
    expect(await client.arch()).toEqual({ modules: 2 });
    expect(await client.status()).toEqual({ ready: true });
    expect(await client.index("/repo")).toEqual({ indexed: true });
    expect(await client.projects()).toHaveLength(1);

    await daemon.close();
  });

  it("falls back only when daemon connection cannot be established", async () => {
    const gcalHome = await temporaryDirectory();
    const endpoint =
      process.platform === "win32"
        ? `\\\\.\\pipe\\gcal-missing-${randomUUID()}`
        : join(gcalHome, "missing.sock");
    const warning = vi.fn();
    const status = vi.fn().mockResolvedValue({ direct: true });
    const fallbackFactory = vi.fn(() => fakeBackend({ status }));
    const client = new DaemonGcalBackendClient({
      gcalHome,
      endpoint,
      command: "goldeneye",
      project: "project-a",
      idleTimeoutMs: 600_000,
      connectTimeoutMs: 20,
      startDaemon: () => undefined,
      fallbackFactory,
      writeWarning: warning,
    });

    expect(await client.status()).toEqual({ direct: true });
    expect(await client.status()).toEqual({ direct: true });
    expect(fallbackFactory).toHaveBeenCalledOnce();
    expect(warning).toHaveBeenCalledOnce();
  });
});

function remoteClient(gcalHome: string, endpoint: string): DaemonGcalBackendClient {
  return new DaemonGcalBackendClient({
    gcalHome,
    endpoint,
    command: "goldeneye",
    project: "project-a",
    idleTimeoutMs: 100,
    startDaemon: () => {
      throw new Error("daemon should already be running");
    },
  });
}

function fakeBackend(overrides: Partial<GcalBackendClient> = {}): GcalBackendClient {
  const unsupported = async () => {
    throw new Error("unexpected backend call");
  };
  return {
    search: unsupported,
    symbol: unsupported,
    get: unsupported,
    callers: unsupported,
    callees: unsupported,
    arch: unsupported,
    status: unsupported,
    index: unsupported,
    projects: unsupported,
    ...overrides,
  };
}

function candidate(qualifiedName: string) {
  return {
    qualifiedName,
    label: "Class" as const,
    filePath: "src/Example.ts",
    line: 1,
    signature: `class ${qualifiedName}`,
  };
}

function selectedSymbol(qualifiedName: string, source: string) {
  return {
    qualifiedName,
    kind: "Class" as const,
    filePath: "src/Example.ts",
    startLine: 1,
    endLine: 1,
    lines: 1,
    complexity: 0,
    cognitive: 0,
    visibility: "public",
    signature: `class ${qualifiedName}`,
    returnType: "",
    decorators: "",
    callers: 0,
    callees: 0,
    source,
  };
}

function traceEdge(sourceQualifiedName: string, targetQualifiedName: string) {
  return {
    sourceQualifiedName,
    targetQualifiedName,
    relatedQualifiedName: targetQualifiedName,
    hop: 1,
    filePath: "src/Example.ts",
    line: 1,
  };
}

async function temporaryDirectory(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "gcal-daemon-"));
  temporaryDirectories.push(directory);
  return directory;
}
