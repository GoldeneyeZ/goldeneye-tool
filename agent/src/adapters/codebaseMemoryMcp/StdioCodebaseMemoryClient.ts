import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import type { SearchOptions, TraceEdge, TraceOptions } from "../../domain/types.js";
import type { GcalBackendClient } from "../../domain/GcalBackendClient.js";
import {
  CodebaseMemoryMcpClient,
  type McpToolInvoker,
} from "./CodebaseMemoryMcpClient.js";
import { unwrapMcpPayload } from "./gatewayJsonRpc.js";

interface JsonRpcRequest {
  jsonrpc: "2.0";
  id?: number;
  method: string;
  params?: Record<string, unknown>;
}

interface PendingRequest {
  resolve(value: unknown): void;
  reject(error: Error): void;
}

const CLOSE_TIMEOUT_MS = 5_000;
const STDERR_TAIL_BYTES = 8_192;

export interface StdioClientConfig {
  command: string;
  project: string;
  env?: NodeJS.ProcessEnv;
  spawn?: typeof spawn;
}

class StdioJsonRpcSession implements McpToolInvoker {
  private child: ChildProcessWithoutNullStreams | undefined;
  private initialized: Promise<void> | undefined;
  private closePromise: Promise<void> | undefined;
  private nextRequestId = 1;
  private stdoutBuffer = "";
  private stderrTail = "";
  private readonly pending = new Map<number, PendingRequest>();

  constructor(
    private readonly command: string,
    private readonly spawnProcess: typeof spawn,
    private readonly env: NodeJS.ProcessEnv | undefined,
  ) {}

  async invoke(toolName: string, args: Record<string, unknown>): Promise<unknown> {
    await this.ensureInitialized();
    return this.request("tools/call", { name: toolName, arguments: args });
  }

  close(): Promise<void> {
    if (this.closePromise) return this.closePromise;

    const child = this.child;
    if (!child) return Promise.resolve();

    this.closePromise = new Promise((resolve) => {
      const timer = setTimeout(() => child.kill(), CLOSE_TIMEOUT_MS);
      child.once("close", () => {
        clearTimeout(timer);
        resolve();
      });
      child.stdin.end();
    });
    return this.closePromise;
  }

  private ensureInitialized(): Promise<void> {
    if (!this.initialized) {
      this.initialized = this.initialize().catch((error: unknown) => {
        this.initialized = undefined;
        throw error;
      });
    }

    return this.initialized;
  }

  private async initialize(): Promise<void> {
    this.startChild();
    await this.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "goldeneye-code-agent-layer", version: "0.1.0" },
    });
    this.notify("notifications/initialized");
  }

  private startChild(): void {
    if (this.child) return;

    const child = this.spawnProcess(this.command, [], {
      stdio: ["pipe", "pipe", "pipe"],
      ...(this.env === undefined ? {} : { env: this.env }),
    });
    this.child = child;
    child.stdout.on("data", (chunk: Buffer) => this.onStdout(chunk));
    child.stderr.on("data", (chunk: Buffer) => this.onStderr(chunk));
    child.on("error", (error) => this.fail(new Error(`MCP process error: ${error.message}`)));
    child.on("exit", (code, signal) =>
      this.fail(this.processTerminationError("exited", code, signal)),
    );
    child.on("close", (code, signal) =>
      this.fail(this.processTerminationError("closed", code, signal)),
    );
  }

  private onStderr(chunk: Buffer): void {
    this.stderrTail = `${this.stderrTail}${chunk.toString()}`.slice(-STDERR_TAIL_BYTES);
  }

  private processTerminationError(
    event: "exited" | "closed",
    code: number | null,
    signal: NodeJS.Signals | null,
  ): Error {
    const status = `code=${code ?? "null"} signal=${signal ?? "null"}`;
    const stderr = this.stderrTail.trim();
    return new Error(
      `MCP process ${event} before response (${status})${stderr ? `: ${stderr}` : ""}`,
    );
  }

  private onStdout(chunk: Buffer): void {
    this.stdoutBuffer += chunk.toString();
    const lines = this.stdoutBuffer.split("\n");
    this.stdoutBuffer = lines.pop() ?? "";

    for (const line of lines) {
      this.onLine(line);
    }
  }

  private onLine(line: string): void {
    let response: unknown;
    try {
      response = JSON.parse(line);
    } catch {
      return;
    }

    if (!isRecord(response) || typeof response.id !== "number") return;

    const pending = this.pending.get(response.id);
    if (!pending) return;
    this.pending.delete(response.id);

    if ("error" in response) {
      pending.reject(new Error(`MCP error: ${errorMessage(response.error)}`));
      return;
    }

    if (!("result" in response)) {
      pending.reject(new Error("MCP response did not include a result"));
      return;
    }

    try {
      pending.resolve(unwrapMcpPayload(response.result));
    } catch (error) {
      pending.reject(toError(error));
    }
  }

  private request(method: string, params?: Record<string, unknown>): Promise<unknown> {
    const id = this.nextRequestId++;
    return this.send({ jsonrpc: "2.0", id, method, params });
  }

  private notify(method: string, params?: Record<string, unknown>): void {
    this.write({ jsonrpc: "2.0", method, params });
  }

  private send(request: JsonRpcRequest): Promise<unknown> {
    if (request.id === undefined) {
      throw new Error("MCP requests require a numeric ID");
    }

    return new Promise((resolve, reject) => {
      this.pending.set(request.id as number, { resolve, reject });
      try {
        this.write(request);
      } catch (error) {
        this.pending.delete(request.id as number);
        reject(toError(error));
      }
    });
  }

  private write(request: JsonRpcRequest): void {
    const child = this.child;
    if (!child) throw new Error("MCP process is not running");
    child.stdin.write(`${JSON.stringify(withoutUndefined(request))}\n`);
  }

  private fail(error: Error): void {
    this.child = undefined;
    this.initialized = undefined;
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }
}

export class StdioCodebaseMemoryClient implements GcalBackendClient {
  protected readonly client: CodebaseMemoryMcpClient;
  private readonly session: StdioJsonRpcSession;

  constructor(config: StdioClientConfig) {
    this.session = new StdioJsonRpcSession(
      config.command,
      config.spawn ?? spawn,
      config.env,
    );
    this.client = new CodebaseMemoryMcpClient(config.project, this.session);
  }

  search(query: string, options: Partial<SearchOptions>) {
    return this.client.search(query, options);
  }

  symbol(nameRegex: string, options: Partial<SearchOptions>) {
    return this.client.symbol(nameRegex, options);
  }

  get(qualifiedName: string) {
    return this.client.get(qualifiedName);
  }

  callers(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.client.callers(qualifiedName, options);
  }

  callees(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.client.callees(qualifiedName, options);
  }

  arch(): Promise<unknown> {
    return this.client.arch();
  }

  status(): Promise<unknown> {
    return this.client.status();
  }

  index(repoPath: string): Promise<unknown> {
    return this.client.index(repoPath);
  }

  projects() {
    return this.client.projects();
  }

  close(): Promise<void> {
    return this.session.close();
  }
}

function withoutUndefined<T extends object>(value: T): T {
  return Object.fromEntries(
    Object.entries(value).filter(([, property]) => property !== undefined),
  ) as T;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function errorMessage(error: unknown): string {
  if (isRecord(error) && typeof error.message === "string") return error.message;
  if (typeof error === "string") return error;
  return "unknown error";
}

function toError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}
