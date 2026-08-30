import { randomUUID } from "node:crypto";
import { createConnection, type Socket } from "node:net";
import type { GcalBackendClient } from "../../domain/GcalBackendClient.js";
import type {
  IndexedProject,
  SearchOptions,
  SelectedSymbol,
  SnippetChunkOptions,
  SnippetManifest,
  SymbolCandidate,
  TraceEdge,
  TraceOptions,
} from "../../domain/types.js";
import {
  DAEMON_PROTOCOL_VERSION,
  type DaemonMethod,
  type DaemonRequest,
  type DaemonResponse,
  invokeBackend,
  responseError,
} from "./daemonProtocol.js";
import { daemonEndpoint, startDetachedDaemon } from "./daemonEndpoint.js";

const MAX_RESPONSE_BYTES = 16 * 1024 * 1024;

export class DaemonUnavailableError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "DaemonUnavailableError";
  }
}

export interface DaemonGcalBackendClientOptions {
  gcalHome: string;
  command: string;
  project: string;
  idleTimeoutMs: number;
  endpoint?: string;
  connectTimeoutMs?: number;
  startDaemon?: () => void | Promise<void>;
  fallbackFactory?: () => GcalBackendClient;
  writeWarning?: (message: string) => void;
}

export class DaemonGcalBackendClient implements GcalBackendClient {
  private readonly endpoint: string;
  private directClient: GcalBackendClient | undefined;
  private warned = false;

  constructor(private readonly options: DaemonGcalBackendClientOptions) {
    this.endpoint = options.endpoint ?? daemonEndpoint(options.gcalHome);
  }

  search(query: string, options: Partial<SearchOptions>) {
    return this.invoke<SymbolCandidate[]>("search", [query, options]);
  }

  symbol(nameRegex: string, options: Partial<SearchOptions>) {
    return this.invoke<SymbolCandidate[]>("symbol", [nameRegex, options]);
  }

  get(qualifiedName: string) {
    return this.invoke<SelectedSymbol>("get", [qualifiedName]);
  }

  getSnippetManifest(qualifiedName: string, chunkBytes: number) {
    return this.invoke<SnippetManifest>("getSnippetManifest", [qualifiedName, chunkBytes]);
  }

  getSnippetChunk(qualifiedName: string, options: SnippetChunkOptions) {
    return this.invoke<SelectedSymbol>("getSnippetChunk", [qualifiedName, options]);
  }

  callers(qualifiedName: string, options: TraceOptions) {
    return this.invoke<TraceEdge[]>("callers", [qualifiedName, options]);
  }

  callees(qualifiedName: string, options: TraceOptions) {
    return this.invoke<TraceEdge[]>("callees", [qualifiedName, options]);
  }

  arch() {
    return this.invoke<unknown>("arch", []);
  }

  status() {
    return this.invoke<unknown>("status", []);
  }

  index(repoPath: string) {
    return this.invoke<unknown>("index", [repoPath]);
  }

  projects() {
    return this.invoke<IndexedProject[]>("projects", []);
  }

  async close(): Promise<void> {
    await this.directClient?.close?.();
  }

  private async invoke<T>(method: DaemonMethod, args: unknown[]): Promise<T> {
    if (this.directClient) {
      return (await invokeBackend(this.directClient, method, args)) as T;
    }

    try {
      const socket = await this.connect();
      return (await sendRequest(socket, {
        id: randomUUID(),
        version: DAEMON_PROTOCOL_VERSION,
        command: this.options.command,
        project: this.options.project,
        method,
        args,
      } as DaemonRequest)) as T;
    } catch (error) {
      if (!(error instanceof DaemonUnavailableError) || !this.options.fallbackFactory) {
        throw error;
      }

      this.directClient = this.options.fallbackFactory();
      if (!this.warned) {
        this.warned = true;
        this.options.writeWarning?.(
          `GCAL daemon unavailable; using direct Goldeneye process: ${error.message}\n`,
        );
      }
      return (await invokeBackend(this.directClient, method, args)) as T;
    }
  }

  private async connect(): Promise<Socket> {
    try {
      return await connectOnce(this.endpoint);
    } catch {
      try {
        await (this.options.startDaemon ?? (() => this.startDefaultDaemon()))();
      } catch (error) {
        throw new DaemonUnavailableError(
          `could not start daemon: ${error instanceof Error ? error.message : String(error)}`,
        );
      }
    }

    const timeoutMs = this.options.connectTimeoutMs ?? 2_000;
    const deadline = Date.now() + timeoutMs;
    let lastError: unknown;
    while (Date.now() < deadline) {
      try {
        return await connectOnce(this.endpoint);
      } catch (error) {
        lastError = error;
        await delay(50);
      }
    }

    throw new DaemonUnavailableError(
      `could not connect to ${this.endpoint}: ${
        lastError instanceof Error ? lastError.message : String(lastError)
      }`,
    );
  }

  private startDefaultDaemon(): void {
    startDetachedDaemon({
      gcalHome: this.options.gcalHome,
      endpoint: this.endpoint,
      idleTimeoutMs: this.options.idleTimeoutMs,
    });
  }
}

function connectOnce(endpoint: string): Promise<Socket> {
  return new Promise((resolve, reject) => {
    const socket = createConnection(endpoint);
    const onError = (error: Error) => reject(error);
    socket.once("error", onError);
    socket.once("connect", () => {
      socket.off("error", onError);
      resolve(socket);
    });
  });
}

function sendRequest(socket: Socket, request: DaemonRequest): Promise<unknown> {
  return new Promise((resolve, reject) => {
    let buffer = "";
    let settled = false;

    const fail = (error: Error) => {
      if (settled) return;
      settled = true;
      socket.destroy();
      reject(error);
    };

    socket.setEncoding("utf8");
    socket.on("data", (chunk: string) => {
      buffer += chunk;
      if (Buffer.byteLength(buffer) > MAX_RESPONSE_BYTES) {
        fail(new Error("GCAL daemon response exceeded 16 MiB"));
        return;
      }

      const newline = buffer.indexOf("\n");
      if (newline < 0) return;
      try {
        const response = JSON.parse(buffer.slice(0, newline)) as DaemonResponse;
        if (response.id !== request.id) {
          fail(new Error("GCAL daemon response id mismatch"));
          return;
        }
        settled = true;
        socket.end();
        if (response.ok) resolve(response.result);
        else reject(responseError(response));
      } catch (error) {
        fail(error instanceof Error ? error : new Error(String(error)));
      }
    });
    socket.once("error", fail);
    socket.once("close", () => {
      if (!settled) fail(new Error("GCAL daemon closed before responding"));
    });
    socket.write(`${JSON.stringify(request)}\n`);
  });
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
