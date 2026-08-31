import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { GcalBackendError, type GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import { createProgram } from "../src/cli/createProgram.js";
import { runCli } from "../src/cli/runCli.js";
import type { TraceEdge } from "../src/domain/types.js";
import {
  normalizeSearchResponse,
  normalizeSelectedSymbol,
} from "../src/adapters/codebaseMemoryMcp/normalize.js";
import {
  largeMethodSnippetResponse,
  methodSnippetResponse,
  searchGraphResponse,
} from "./fixtures/codebaseMemory.js";

function fakeClient(overrides: Partial<GcalBackendClient> = {}): GcalBackendClient {
  return {
    search: vi.fn(),
    symbol: vi.fn(),
    get: vi.fn(),
    callers: vi.fn(),
    callees: vi.fn(),
    arch: vi.fn(),
    status: vi.fn(),
    index: vi.fn(),
    projects: vi.fn(),
    ...overrides,
  };
}

function createTestProgram(
  client: GcalBackendClient,
  initProject: (repoPath: string) => Promise<{ project: string; rootPath: string }> = vi.fn(),
) {
  const writes: string[] = [];
  const errors: string[] = [];
  const program = createProgram({
    client,
    initProject,
    writeOut: (text) => writes.push(text),
    writeErr: (text) => errors.push(text),
  });

  return { errors, program, writes };
}

describe("GCAL CLI", () => {
  const inboundTrace: TraceEdge[] = [
    {
      sourceQualifiedName: "com.example.booking.BookingController.cancelBooking",
      targetQualifiedName: "com.example.booking.BookingService.cancelBooking",
      relatedQualifiedName: "com.example.booking.BookingController.cancelBooking",
      hop: 1,
      filePath: "src/main/java/com/example/booking/BookingController.java",
      line: 31,
    },
  ];
  const outboundTrace: TraceEdge[] = [
    {
      sourceQualifiedName: "com.example.booking.BookingService.cancelBooking",
      targetQualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
      relatedQualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
      hop: 1,
      filePath: "src/main/java/com/example/booking/BookingRepository.java",
      line: 73,
    },
  ];

  it("renders subcommand help without GCAL_PROJECT or client construction", async () => {
    const writes: string[] = [];
    const errors: string[] = [];
    const createClient = vi.fn(() => fakeClient());

    const exitCode = await runCli({
      argv: ["node", "gcal", "help", "search"],
      env: {},
      resolveProject: async () => undefined,
      createClient,
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(0);
    expect(createClient).not.toHaveBeenCalled();
    expect(errors.join("")).toBe("");
    expect(writes.join("")).toContain("Usage: gcal search [options] <query>");
  });

  it("uses Goldeneye by default when index runs without GCAL_PROJECT", async () => {
    const writes: string[] = [];
    const index = vi.fn().mockResolvedValue({ indexed: true, repo_path: "." });
    const createClient = vi.fn(() => fakeClient({ index }));

    const exitCode = await runCli({
      argv: ["node", "gcal", "index", "."],
      env: {},
      resolveProject: async () => undefined,
      createClient,
      writeOut: (text) => writes.push(text),
      writeErr: () => undefined,
    });

    expect(exitCode).toBe(0);
    expect(createClient).toHaveBeenCalledWith({
      backend: "goldeneye",
      command: "goldeneye",
      mcpUrl: undefined,
      project: "",
    });
    expect(index).toHaveBeenCalledWith(".");
    expect(writes.join("")).toBe('{"indexed":true,"repo_path":"."}\n');
  });

  it("uses the compatibility adapter only in explicit benchmark mode", async () => {
    const createClient = vi.fn(() => fakeClient({ status: vi.fn().mockResolvedValue({}) }));

    const exitCode = await runCli({
      argv: ["node", "gcal", "status"],
      env: {
        GCAL_BACKEND: "benchmark",
        GCAL_MCP_COMMAND: "C:\\tools\\codebase-memory-mcp.exe",
        GCAL_MCP_URL: "http://gateway.example/mcp",
        GCAL_PROJECT: "example-project",
      },
      createClient,
      writeOut: () => undefined,
      writeErr: () => undefined,
    });

    expect(exitCode).toBe(0);
    expect(createClient).toHaveBeenCalledWith({
      backend: "benchmark",
      command: "C:\\tools\\codebase-memory-mcp.exe",
      mcpUrl: "http://gateway.example/mcp",
      project: "example-project",
    });
  });

  it("ignores stale compatibility settings when Goldeneye is selected", async () => {
    const createClient = vi.fn(() => fakeClient({ status: vi.fn().mockResolvedValue({}) }));

    const exitCode = await runCli({
      argv: ["node", "gcal", "status"],
      env: {
        GCAL_GOLDENEYE_COMMAND: "C:\\tools\\goldeneye.exe",
        GCAL_MCP_COMMAND: "C:\\legacy\\codebase-memory-mcp.exe",
        GCAL_MCP_URL: "http://legacy.example/mcp",
        GCAL_PROJECT: "example-project",
      },
      createClient,
      writeOut: () => undefined,
      writeErr: () => undefined,
    });

    expect(exitCode).toBe(0);
    expect(createClient).toHaveBeenCalledWith({
      backend: "goldeneye",
      command: "C:\\tools\\goldeneye.exe",
      mcpUrl: undefined,
      project: "example-project",
    });
  });

  it("rejects an unknown backend", async () => {
    const errors: string[] = [];
    const createClient = vi.fn(() => fakeClient());

    const exitCode = await runCli({
      argv: ["node", "gcal", "index", "."],
      env: { GCAL_BACKEND: "other" },
      createClient,
      writeOut: () => undefined,
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(2);
    expect(createClient).not.toHaveBeenCalled();
    expect(errors.join("")).toBe("GCAL_BACKEND must be 'goldeneye' or 'benchmark'\n");
  });

  it("requires a command for explicit benchmark mode", async () => {
    const errors: string[] = [];
    const createClient = vi.fn(() => fakeClient());

    const exitCode = await runCli({
      argv: ["node", "gcal", "index", "."],
      env: { GCAL_BACKEND: "benchmark" },
      createClient,
      writeOut: () => undefined,
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(2);
    expect(createClient).not.toHaveBeenCalled();
    expect(errors.join("")).toBe("GCAL_MCP_COMMAND is required when GCAL_BACKEND=benchmark\n");
  });

  it("awaits an optional client close after command execution", async () => {
    let resolveClose: (() => void) | undefined;
    const close = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveClose = resolve;
        }),
    );
    const client = Object.assign(fakeClient({ status: vi.fn().mockResolvedValue({}) }), { close });

    const result = runCli({
      argv: ["node", "gcal", "status"],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => client,
      writeOut: () => undefined,
      writeErr: () => undefined,
    });

    await vi.waitFor(() => expect(close).toHaveBeenCalledOnce());
    resolveClose?.();
    expect(await result).toBe(0);
  });

  it("uses a project registered for the current directory", async () => {
    const status = vi.fn().mockResolvedValue({ status: "ready" });
    const createClient = vi.fn(() => fakeClient({ status }));

    const exitCode = await runCli({
      argv: ["node", "gcal", "status"],
      env: {},
      currentDirectory: "C:\\code\\example\\src",
      homeDir: "C:\\Users\\example",
      resolveProject: async () => "registered-project",
      createClient,
      writeOut: () => undefined,
      writeErr: () => undefined,
    });

    expect(exitCode).toBe(0);
    expect(createClient).toHaveBeenCalledWith({
      backend: "goldeneye",
      command: "goldeneye",
      mcpUrl: undefined,
      project: "registered-project",
    });
  });

  it("suggests init when the current directory is not registered", async () => {
    const errors: string[] = [];
    const createClient = vi.fn(() => fakeClient());

    const exitCode = await runCli({
      argv: ["node", "gcal", "status"],
      env: {},
      currentDirectory: "C:\\code\\unregistered",
      resolveProject: async () => undefined,
      createClient,
      writeOut: () => undefined,
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(2);
    expect(createClient).not.toHaveBeenCalled();
    expect(errors.join("")).toBe(
      "No GCAL project registered for C:\\code\\unregistered; run 'gcal init' in the project root\n",
    );
  });

  it("does not create a client for unregistered inspect", async () => {
    const errors: string[] = [];
    const createClient = vi.fn(() => fakeClient());

    const exitCode = await runCli({
      argv: ["node", "gcal", "inspect", "BookingService"],
      env: {},
      currentDirectory: "C:\\code\\unregistered",
      resolveProject: async () => undefined,
      createClient,
      writeOut: () => undefined,
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(2);
    expect(createClient).not.toHaveBeenCalled();
    expect(errors.join("")).toContain("run 'gcal init'");
  });

  it("indexes and registers the current project with init", async () => {
    const stateRoot = await mkdtemp(join(tmpdir(), "gcal-state-"));
    const gcalHome = join(stateRoot, "isolated-gcal-home");
    const projectRoot = await mkdtemp(join(tmpdir(), "gcal-project-"));
    const index = vi.fn().mockResolvedValue({ indexed: true });
    const projects = vi
      .fn()
      .mockResolvedValue([{ name: "registered-project", rootPath: projectRoot }]);
    const createClient = vi.fn(() => fakeClient({ index, projects }));
    const writes: string[] = [];

    try {
      const exitCode = await runCli({
        argv: ["node", "gcal", "init"],
        env: { GCAL_HOME: gcalHome },
        currentDirectory: projectRoot,
        createClient,
        writeOut: (text) => writes.push(text),
        writeErr: () => undefined,
      });

      expect(exitCode).toBe(0);
      expect(index).toHaveBeenCalledWith(projectRoot);
      expect(projects).toHaveBeenCalledOnce();
      expect(writes.join("")).toBe(
        `${JSON.stringify({ project: "registered-project", root_path: projectRoot })}\n`,
      );
      await expect(readFile(join(gcalHome, "projects.json"), "utf8")).resolves.toContain(
        '"project":"registered-project"',
      );
    } finally {
      await Promise.all([
        rm(stateRoot, { recursive: true, force: true }),
        rm(projectRoot, { recursive: true, force: true }),
      ]);
    }
  });

  it("prints search rows using an injected client", async () => {
    const search = vi.fn().mockResolvedValue(normalizeSearchResponse(searchGraphResponse));
    const { program, writes } = createTestProgram(fakeClient({ search }));

    await program.parseAsync(["node", "gcal", "search", "BookingService", "--limit", "5"]);

    expect(search).toHaveBeenCalledWith("BookingService", {
      limit: 5,
      label: undefined,
      filePattern: undefined,
      qualifiedNamePattern: undefined,
    });
    expect(writes.join("")).toBe(
      "com.example.booking.BookingService.cancelBooking\tMethod\tsrc/main/java/com/example/booking/BookingService.java\t42\tpublic BookingResponse cancelBooking(String bookingId)\n",
    );
  });

  it("runs ordered multi-query search with default top-three snippet hydration", async () => {
    const first = normalizeSearchResponse(searchGraphResponse)[0];
    const second = {
      ...first,
      qualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
      signature: "Booking findActiveBooking()",
    };
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const search = vi.fn().mockResolvedValueOnce([first]).mockResolvedValueOnce([first, second]);
    const get = vi.fn(async (qualifiedName: string) => ({
      ...selected,
      qualifiedName,
      source: `source:${qualifiedName}`,
    }));
    const { errors, program, writes } = createTestProgram(fakeClient({ get, search }));

    await program.parseAsync([
      "node",
      "gcal",
      "search",
      "BookingService",
      "--query",
      "BookingRepository",
      "--snippets",
    ]);

    expect(search.mock.calls.map(([query]) => query)).toEqual([
      "BookingService",
      "BookingRepository",
    ]);
    expect(get.mock.calls).toEqual([[first.qualifiedName], [second.qualifiedName]]);
    expect(writes.join("")).toContain(`# snippet\t${first.qualifiedName}\n`);
    expect(writes.join("")).toContain(`# snippet\t${second.qualifiedName}\n`);
    expect(errors).toEqual([]);
  });

  it("returns 1 after preserving multi-query results when query and hydration branches fail", async () => {
    const row = normalizeSearchResponse(searchGraphResponse)[0];
    const search = vi
      .fn()
      .mockResolvedValueOnce([row])
      .mockRejectedValueOnce(new Error("stale\nindex"));
    const get = vi.fn().mockRejectedValue(new Error("oversized"));
    const writes: string[] = [];
    const errors: string[] = [];

    const exitCode = await runCli({
      argv: ["node", "gcal", "search", "BookingService", "--query", "Broken", "--snippets", "1"],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => fakeClient({ get, search }),
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(writes.join("")).toContain(row.qualifiedName);
    expect(errors.join("")).toBe(
      'gcal: search query failed "Broken": stale index\n' +
        `gcal: search snippet failed "${row.qualifiedName}": oversized\n`,
    );
  });

  it("rejects snippet hydration above five before search", async () => {
    const search = vi.fn();
    const writes: string[] = [];
    const errors: string[] = [];

    const exitCode = await runCli({
      argv: ["node", "gcal", "search", "BookingService", "--snippets", "6"],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => fakeClient({ search }),
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(search).not.toHaveBeenCalled();
    expect(writes).toEqual([]);
    expect(errors.join("")).toBe("gcal search --snippets accepts at most 5\n");
  });

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

  it("prints get output as source only using an injected client", async () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const get = vi.fn().mockResolvedValue(selected);
    const { program, writes } = createTestProgram(fakeClient({ get }));

    await program.parseAsync([
      "node",
      "gcal",
      "get",
      "com.example.booking.BookingService.cancelBooking",
    ]);

    expect(get).toHaveBeenCalledWith("com.example.booking.BookingService.cancelBooking");
    expect(writes.join("")).toBe(`${selected.source}\n`);
  });

  it("turns typed oversized single get into one manifest call and compact chunk UX", async () => {
    const qualifiedName = "com.example.Huge.run";
    const get = vi.fn().mockRejectedValue(
      new GcalBackendError("MCP tool error: snippet exceeds bounds", "SnippetTooLarge", {
        actual_lines: 900,
      }),
    );
    const getSnippetChunk = vi.fn();
    const getSnippetManifest = vi.fn().mockResolvedValue({
      selected: {
        ...normalizeSelectedSymbol(methodSnippetResponse),
        qualifiedName,
        source: "",
      },
      sourceBytes: 24_000,
      sourceLines: 900,
      sourceSha256: "a".repeat(64),
      indexedFileHash: "indexed",
      chunkBytes: 8_192,
      chunkCount: 3,
    });
    const { program, writes } = createTestProgram(
      fakeClient({ get, getSnippetChunk, getSnippetManifest }),
    );

    await program.parseAsync(["node", "gcal", "get", qualifiedName]);

    expect(get).toHaveBeenCalledTimes(1);
    expect(getSnippetManifest).toHaveBeenCalledWith(qualifiedName, 8_192);
    expect(getSnippetChunk).not.toHaveBeenCalled();
    expect(writes.join("")).toBe(
      `snippet-too-large\t${qualifiedName}\n` +
        "bytes\t24000\n" +
        "lines\t900\n" +
        "chunks\t3\n" +
        "chunk-bytes\t8192\n" +
        `source-sha256\t${"a".repeat(64)}\n` +
        `next\tack get ${qualifiedName} --chunk 1 --expected-source-sha ${"a".repeat(64)}\n`,
    );
  });

  it("uses exactly one direct chunk call with locally validated source SHA", async () => {
    const qualifiedName = "com.example.Huge.run";
    const sha = "b".repeat(64);
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const get = vi.fn();
    const getSnippetManifest = vi.fn();
    const getSnippetChunk = vi.fn().mockResolvedValue(selected);
    const { program, writes } = createTestProgram(
      fakeClient({ get, getSnippetChunk, getSnippetManifest }),
    );

    await program.parseAsync([
      "node",
      "gcal",
      "get",
      qualifiedName,
      "--chunk",
      "1",
      "--expected-source-sha",
      sha,
    ]);

    expect(getSnippetChunk).toHaveBeenCalledWith(qualifiedName, {
      chunk: 1,
      chunkBytes: 8_192,
      expectedSourceSha256: sha,
    });
    expect(get).not.toHaveBeenCalled();
    expect(getSnippetManifest).not.toHaveBeenCalled();
    expect(writes.join("")).toBe(`${selected.source}\n`);
  });

  it.each([
    {
      args: ["--chunk", "1", "--expected-source-sha", "A".repeat(64)],
      message: "gcal get --expected-source-sha must be exactly 64 lowercase hexadecimal characters",
    },
    {
      args: ["--expected-source-sha", "a".repeat(64)],
      message: "gcal get --expected-source-sha requires --chunk",
    },
  ])("rejects invalid local chunk options before backend calls", async ({ args, message }) => {
    const get = vi.fn();
    const getSnippetChunk = vi.fn();
    const writes: string[] = [];
    const errors: string[] = [];

    const exitCode = await runCli({
      argv: ["node", "gcal", "get", "com.example.Huge.run", ...args],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => fakeClient({ get, getSnippetChunk }),
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(get).not.toHaveBeenCalled();
    expect(getSnippetChunk).not.toHaveBeenCalled();
    expect(writes).toEqual([]);
    expect(errors.join("")).toBe(`${message}\n`);
  });

  it("preserves typed stale-source failures from one explicit chunk call", async () => {
    const sha = "c".repeat(64);
    const getSnippetChunk = vi.fn().mockRejectedValue(
      new GcalBackendError("MCP tool error: snippet source changed", "StaleSnippetSource", {
        expected_source_sha256: sha,
      }),
    );
    const writes: string[] = [];
    const errors: string[] = [];

    const exitCode = await runCli({
      argv: [
        "node",
        "gcal",
        "get",
        "com.example.Huge.run",
        "--chunk",
        "1",
        "--expected-source-sha",
        sha,
      ],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => fakeClient({ getSnippetChunk }),
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(getSnippetChunk).toHaveBeenCalledTimes(1);
    expect(writes).toEqual([]);
    expect(errors.join("")).toBe("MCP tool error: snippet source changed\n");
  });

  it("preserves non-oversized single-get failures unchanged", async () => {
    const ordinary = new Error("MCP tool error: symbol missing");
    const getSnippetManifest = vi.fn();
    const errors: string[] = [];
    const exitCode = await runCli({
      argv: ["node", "gcal", "get", "com.example.Missing.run"],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () =>
        fakeClient({
          get: vi.fn().mockRejectedValue(ordinary),
          getSnippetManifest,
        }),
      writeOut: vi.fn(),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(getSnippetManifest).not.toHaveBeenCalled();
    expect(errors.join("")).toBe("MCP tool error: symbol missing\n");
  });

  it("prints bounded batch get output in input order", async () => {
    const first = normalizeSelectedSymbol(methodSnippetResponse);
    const second = {
      ...first,
      qualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
      source: "public Booking findActiveBooking() {}",
    };
    const get = vi.fn().mockResolvedValueOnce(first).mockResolvedValueOnce(second);
    const { errors, program, writes } = createTestProgram(fakeClient({ get }));

    await program.parseAsync(["node", "gcal", "get", first.qualifiedName, second.qualifiedName]);

    expect(get.mock.calls).toEqual([[first.qualifiedName], [second.qualifiedName]]);
    expect(writes.join("")).toBe(
      `# ${first.qualifiedName}\n${first.source}\n\n` +
        `# ${second.qualifiedName}\n${second.source}\n`,
    );
    expect(errors).toEqual([]);
  });

  it("returns 1 for partial batch get failure after preserving successful output", async () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const get = vi
      .fn()
      .mockResolvedValueOnce(selected)
      .mockRejectedValueOnce(new Error("missing\nsymbol"));
    const writes: string[] = [];
    const errors: string[] = [];

    const exitCode = await runCli({
      argv: ["node", "gcal", "get", selected.qualifiedName, "missing.Symbol"],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => fakeClient({ get }),
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(writes.join("")).toBe(`# ${selected.qualifiedName}\n${selected.source}\n`);
    expect(errors.join("")).toBe("gcal: get failed missing.Symbol: missing symbol\n");
  });

  it("rejects more than 32 batch get IDs before fetching", async () => {
    const get = vi.fn();
    const writes: string[] = [];
    const errors: string[] = [];
    const qualifiedNames = Array.from({ length: 33 }, (_, index) => `symbol-${index}`);

    const exitCode = await runCli({
      argv: ["node", "gcal", "get", ...qualifiedNames],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => fakeClient({ get }),
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(get).not.toHaveBeenCalled();
    expect(writes).toEqual([]);
    expect(errors.join("")).toBe("gcal get accepts at most 32 symbols per batch\n");
  });

  it("inspects exact qualified names directly with metadata and traces but no source", async () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const search = vi.fn().mockResolvedValue([]);
    const get = vi.fn().mockResolvedValue(selected);
    const callers = vi.fn().mockResolvedValue(inboundTrace);
    const callees = vi.fn().mockResolvedValue(outboundTrace);
    const { program, writes } = createTestProgram(fakeClient({ callees, callers, get, search }));

    await program.parseAsync([
      "node",
      "gcal",
      "inspect",
      "com.example.booking.BookingService.cancelBooking",
    ]);

    const output = writes.join("");
    expect(search).not.toHaveBeenCalled();
    expect(get).toHaveBeenCalledWith("com.example.booking.BookingService.cancelBooking");
    expect(callers).toHaveBeenCalledWith("com.example.booking.BookingService.cancelBooking", {
      depth: 1,
    });
    expect(callees).toHaveBeenCalledWith("com.example.booking.BookingService.cancelBooking", {
      depth: 1,
    });
    expect(output).toContain("# selected");
    expect(output).toContain("qualified_name=com.example.booking.BookingService.cancelBooking");
    expect(output).toContain("# inbound");
    expect(output).toContain("# outbound");
    expect(output).not.toContain("resolveActiveBooking");
    expect(output).not.toContain(selected.source);
  });

  it("inspects broad queries by printing candidates, selecting first, then getting traces", async () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const firstCandidate = normalizeSearchResponse(searchGraphResponse)[0];
    const secondCandidate = {
      ...firstCandidate,
      qualifiedName: "com.example.booking.BookingService.reconcileBooking",
      line: 91,
      signature: "public BookingResponse reconcileBooking(String bookingId)",
    };
    const search = vi.fn().mockResolvedValue([firstCandidate, secondCandidate]);
    const get = vi.fn().mockResolvedValue(selected);
    const callers = vi.fn().mockResolvedValue(inboundTrace);
    const callees = vi.fn().mockResolvedValue(outboundTrace);
    const { program, writes } = createTestProgram(fakeClient({ callees, callers, get, search }));

    await program.parseAsync(["node", "gcal", "inspect", "BookingService", "--limit", "2"]);

    const output = writes.join("");
    expect(search).toHaveBeenCalledWith("BookingService", { limit: 2 });
    expect(get).toHaveBeenCalledWith(firstCandidate.qualifiedName);
    expect(callers).toHaveBeenCalledWith(selected.qualifiedName, { depth: 1 });
    expect(callees).toHaveBeenCalledWith(selected.qualifiedName, { depth: 1 });
    expect(output).toContain("# candidates");
    expect(output).toContain("1\tMethod\tcom.example.booking.BookingService.cancelBooking");
    expect(output).toContain("2\tMethod\tcom.example.booking.BookingService.reconcileBooking");
    expect(output).toContain("# selected");
    expect(output).toContain("# inbound");
    expect(output).toContain("# outbound");
  });

  it("fails inspect broad queries concisely when no candidates are found", async () => {
    const search = vi.fn().mockResolvedValue([]);
    const get = vi.fn();
    const { program, writes } = createTestProgram(fakeClient({ get, search }));

    await expect(program.parseAsync(["node", "gcal", "inspect", "MissingService"])).rejects.toThrow(
      "inspect found no candidates for MissingService",
    );

    expect(search).toHaveBeenCalledWith("MissingService", { limit: 5 });
    expect(get).not.toHaveBeenCalled();
    expect(writes.join("")).toBe("");
  });

  it("prints affordance warnings when inspecting a large high-caller symbol", async () => {
    const selected = normalizeSelectedSymbol(largeMethodSnippetResponse);
    const get = vi.fn().mockResolvedValue(selected);
    const callers = vi.fn().mockResolvedValue(inboundTrace);
    const callees = vi.fn().mockResolvedValue(outboundTrace);
    const { program, writes } = createTestProgram(fakeClient({ callees, callers, get }));

    await program.parseAsync([
      "node",
      "gcal",
      "inspect",
      "com.example.booking.BookingService.reconcileBooking",
    ]);

    const output = writes.join("");
    expect(output).toContain("large method; source likely needed");
    expect(output).toContain("high complexity; inspect related callers and tests before editing");
    expect(output).toContain("high caller count; use callers command rather than inline trace");
  });

  it("prints an inbound trace hint instead of calling callers when caller count exceeds threshold", async () => {
    const selected = normalizeSelectedSymbol(largeMethodSnippetResponse);
    const get = vi.fn().mockResolvedValue(selected);
    const callers = vi.fn().mockResolvedValue(inboundTrace);
    const callees = vi.fn().mockResolvedValue(outboundTrace);
    const { program, writes } = createTestProgram(fakeClient({ callees, callers, get }));

    await program.parseAsync([
      "node",
      "gcal",
      "inspect",
      "com.example.booking.BookingService.reconcileBooking",
    ]);

    const output = writes.join("");
    expect(callers).not.toHaveBeenCalled();
    expect(callees).toHaveBeenCalledWith(selected.qualifiedName, { depth: 1 });
    expect(output).toContain("# inbound");
    expect(output).toContain(
      "hint\t12\tgcal callers com.example.booking.BookingService.reconcileBooking --depth 1",
    );
    expect(output).toContain("# outbound");
  });

  it("prints standalone caller and callee traces as headerless rows", async () => {
    const inbound: TraceEdge[] = [
      {
        sourceQualifiedName: "com.example.booking.BookingController.cancelBooking",
        targetQualifiedName: "com.example.booking.BookingService.cancelBooking",
        relatedQualifiedName: "com.example.booking.BookingController.cancelBooking",
        hop: 1,
        filePath: "src/main/java/com/example/booking/BookingController.java",
        line: 31,
      },
    ];
    const outbound: TraceEdge[] = [
      {
        sourceQualifiedName: "com.example.booking.BookingService.cancelBooking",
        targetQualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
        relatedQualifiedName: "com.example.booking.BookingRepository.findActiveBooking",
        hop: 1,
        filePath: "src/main/java/com/example/booking/BookingRepository.java",
        line: 73,
      },
    ];
    const callers = vi.fn().mockResolvedValue(inbound);
    const callees = vi.fn().mockResolvedValue(outbound);
    const { program, writes } = createTestProgram(fakeClient({ callers, callees }));

    await program.parseAsync([
      "node",
      "gcal",
      "callers",
      "com.example.booking.BookingService.cancelBooking",
      "--depth",
      "2",
      "--limit",
      "1",
    ]);
    await program.parseAsync([
      "node",
      "gcal",
      "callees",
      "com.example.booking.BookingService.cancelBooking",
    ]);

    expect(callers).toHaveBeenCalledWith("com.example.booking.BookingService.cancelBooking", {
      depth: 2,
    });
    expect(callees).toHaveBeenCalledWith("com.example.booking.BookingService.cancelBooking", {
      depth: 1,
    });
    expect(writes.join("")).toBe(
      [
        "com.example.booking.BookingController.cancelBooking\t1\tsrc/main/java/com/example/booking/BookingController.java\t31",
        "com.example.booking.BookingRepository.findActiveBooking\t1\tsrc/main/java/com/example/booking/BookingRepository.java\t73",
        "",
      ].join("\n"),
    );
  });

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

  it.each([
    ["search rejects numeric prefixes", ["search", "BookingService", "--limit", "1abc"], "search"],
    ["search rejects decimal limits", ["search", "BookingService", "--limit", "1.5"], "search"],
    ["search rejects negative limits", ["search", "BookingService", "--limit", "-1"], "search"],
    [
      "callers rejects numeric prefixes",
      ["callers", "com.example.BookingService.cancelBooking", "--depth", "1abc"],
      "callers",
    ],
    [
      "callers rejects decimal depths",
      ["callers", "com.example.BookingService.cancelBooking", "--depth", "1.5"],
      "callers",
    ],
    [
      "callers rejects negative depths",
      ["callers", "com.example.BookingService.cancelBooking", "--depth", "-1"],
      "callers",
    ],
    [
      "callers rejects numeric limit prefixes",
      ["callers", "com.example.BookingService.cancelBooking", "--limit", "1abc"],
      "callers",
    ],
    [
      "callers rejects decimal limits",
      ["callers", "com.example.BookingService.cancelBooking", "--limit", "1.5"],
      "callers",
    ],
    [
      "callers rejects negative limits",
      ["callers", "com.example.BookingService.cancelBooking", "--limit", "-1"],
      "callers",
    ],
  ])("%s", async (_name, args, methodName) => {
    const client = fakeClient({
      search: vi.fn().mockResolvedValue([]),
      callers: vi.fn().mockResolvedValue([]),
    });
    const { program } = createTestProgram(client);
    program.exitOverride();

    await expect(program.parseAsync(["node", "gcal", ...args])).rejects.toThrow(
      /expected a non-negative integer/,
    );
    expect(client[methodName as "search" | "callers"]).not.toHaveBeenCalled();
  });

  it("accepts zero and default numeric options", async () => {
    const search = vi.fn().mockResolvedValue([]);
    const callers = vi.fn().mockResolvedValue([]);
    const { program } = createTestProgram(fakeClient({ callers, search }));

    await program.parseAsync(["node", "gcal", "search", "BookingService", "--limit", "0"]);
    await program.parseAsync([
      "node",
      "gcal",
      "callers",
      "com.example.BookingService.cancelBooking",
    ]);

    expect(search).toHaveBeenCalledWith("BookingService", {
      limit: 0,
      label: undefined,
      filePattern: undefined,
      qualifiedNamePattern: undefined,
    });
    expect(callers).toHaveBeenCalledWith("com.example.BookingService.cancelBooking", {
      depth: 1,
    });
  });

  it("prints architecture, status, and index responses as compact JSON", async () => {
    const arch = vi.fn().mockResolvedValue({ project: "example-project", modules: [] });
    const status = vi.fn().mockResolvedValue({ indexed: true, symbols: 423 });
    const index = vi.fn().mockResolvedValue({ indexed: true, repo_path: "." });
    const { program, writes } = createTestProgram(fakeClient({ arch, index, status }));

    await program.parseAsync(["node", "gcal", "arch"]);
    await program.parseAsync(["node", "gcal", "status"]);
    await program.parseAsync(["node", "gcal", "index"]);

    expect(index).toHaveBeenCalledWith(".");
    expect(writes.join("")).toBe(
      [
        '{"project":"example-project","modules":[]}',
        '{"indexed":true,"symbols":423}',
        '{"indexed":true,"repo_path":"."}',
        "",
      ].join("\n"),
    );
  });

  it("runs adaptive JavaScript workflow code in one invocation", async () => {
    const candidates = normalizeSearchResponse(searchGraphResponse);
    const search = vi.fn().mockResolvedValue(candidates);
    const { program, writes } = createTestProgram(fakeClient({ search }));

    await program.parseAsync([
      "node",
      "gcal",
      "workflow",
      "--js",
      'const hits = await gcal.search("BookingService"); return hits.map(hit => hit.qualifiedName);',
    ]);

    expect(search).toHaveBeenCalledWith("BookingService", {
      limit: 5,
      label: undefined,
      filePattern: undefined,
      qualifiedNamePattern: undefined,
    });
    expect(writes.join("")).toBe(`${JSON.stringify(candidates.map((row) => row.qualifiedName))}\n`);
  });

  it("runs JavaScript workflow code from a file", async () => {
    const directory = await mkdtemp(join(tmpdir(), "gcal-workflow-"));
    const workflowPath = join(directory, "investigate.js");
    await writeFile(workflowPath, 'return (await gcal.search("token"))[0] ?? null;\n');
    const search = vi.fn().mockResolvedValue([normalizeSearchResponse(searchGraphResponse)[0]]);
    const { program, writes } = createTestProgram(fakeClient({ search }));

    try {
      await program.parseAsync(["node", "gcal", "workflow", "--file", workflowPath]);
      expect(search).toHaveBeenCalledOnce();
      expect(JSON.parse(writes.join(""))).toMatchObject({ qualifiedName: expect.any(String) });
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });

  it("returns exit status 1 when JavaScript workflow code fails", async () => {
    const writes: string[] = [];
    const errors: string[] = [];
    const callers = vi.fn().mockRejectedValue(new Error("caller graph unavailable"));

    const exitCode = await runCli({
      argv: [
        "node",
        "gcal",
        "workflow",
        "--js",
        'return await gcal.callers("com.example.booking.BookingService.cancelBooking");',
      ],
      env: { GCAL_PROJECT: "example-project" },
      createClient: () => fakeClient({ callers }),
      writeOut: (text) => writes.push(text),
      writeErr: (text) => errors.push(text),
    });

    expect(exitCode).toBe(1);
    expect(writes.join("")).toBe("");
    expect(errors.join("")).toContain(
      "gcal workflow JavaScript failed: Error: caller graph unavailable",
    );
  });

  it("registers GCAL commands including init", () => {
    const { program } = createTestProgram(fakeClient());

    expect(program.commands.map((command) => command.name()).sort()).toEqual([
      "arch",
      "callees",
      "callers",
      "get",
      "index",
      "init",
      "inspect",
      "search",
      "status",
      "symbol",
      "workflow",
    ]);
  });
});
