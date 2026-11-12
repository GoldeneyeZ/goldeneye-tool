import { chmod, mkdir, rm } from "node:fs/promises";
import { createConnection, createServer, type Server, type Socket } from "node:net";
import { dirname } from "node:path";
import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import {
  daemonRequestSchema,
  errorResponse,
  invokeBackend,
  type DaemonRequest,
  type DaemonSuccessResponse,
} from "../adapters/daemon/daemonProtocol.js";

const MAX_REQUEST_BYTES = 1024 * 1024;

interface BackendSession {
  client: GcalBackendClient;
  active: number;
  lastUsed: number;
  tail: Promise<void>;
}

export interface StartDaemonServerOptions {
  endpoint: string;
  lockPath: string;
  idleTimeoutMs: number;
  createClient(command: string, project: string): GcalBackendClient;
}

export interface DaemonServer {
  closed: Promise<void>;
  close(): Promise<void>;
}

export async function startDaemonServer(options: StartDaemonServerOptions): Promise<DaemonServer> {
  if (process.platform !== "win32") {
    await mkdir(dirname(options.endpoint), { recursive: true });
    if (await endpointIsActive(options.endpoint)) {
      throw new Error(`GCAL daemon already listening on ${options.endpoint}`);
    }
    await rm(options.endpoint, { force: true });
  }

  const sessions = new Map<string, BackendSession>();
  let lastActivity = Date.now();
  let shutdownPromise: Promise<void> | undefined;
  let resolveClosed!: () => void;
  const closed = new Promise<void>((resolve) => {
    resolveClosed = resolve;
  });

  const server = createServer((socket) => {
    receiveRequest(socket, async (request) => {
      lastActivity = Date.now();
      try {
        const result = await enqueue(request);
        const response: DaemonSuccessResponse = { id: request.id, ok: true, result };
        socket.end(`${JSON.stringify(response)}\n`);
      } catch (error) {
        socket.end(`${JSON.stringify(errorResponse(request.id, error))}\n`);
      } finally {
        lastActivity = Date.now();
      }
    });
  });

  function enqueue(request: DaemonRequest): Promise<unknown> {
    const key = `${request.command}\0${request.project}`;
    let session = sessions.get(key);
    if (!session) {
      session = {
        client: options.createClient(request.command, request.project),
        active: 0,
        lastUsed: Date.now(),
        tail: Promise.resolve(),
      };
      sessions.set(key, session);
    }

    session.active += 1;
    const result = session.tail.then(() =>
      invokeBackend(session!.client, request.method, request.args),
    );
    session.tail = result.then(
      () => undefined,
      () => undefined,
    );
    void result.then(
      () => {
        session!.active -= 1;
        session!.lastUsed = Date.now();
      },
      () => {
        session!.active -= 1;
        session!.lastUsed = Date.now();
      },
    );
    return result;
  }

  async function cleanupIdleSessions(): Promise<void> {
    const now = Date.now();
    for (const [key, session] of sessions) {
      if (session.active > 0 || now - session.lastUsed < options.idleTimeoutMs) continue;
      sessions.delete(key);
      await session.tail;
      await session.client.close?.();
    }

    if (sessions.size === 0 && now - lastActivity >= options.idleTimeoutMs) {
      await shutdown();
    }
  }

  async function shutdown(): Promise<void> {
    if (shutdownPromise) return shutdownPromise;
    shutdownPromise = (async () => {
      clearInterval(cleanupTimer);
      await Promise.all(
        [...sessions.values()].map(async (session) => {
          await session.tail;
          await session.client.close?.();
        }),
      );
      sessions.clear();
      await closeServer(server);
      if (process.platform !== "win32") await rm(options.endpoint, { force: true });
      await rm(options.lockPath, { force: true });
      resolveClosed();
    })();
    return shutdownPromise;
  }

  await listen(server, options.endpoint);
  if (process.platform !== "win32") await chmod(options.endpoint, 0o600);
  await rm(options.lockPath, { force: true });

  const cleanupEveryMs = Math.min(30_000, Math.max(25, Math.floor(options.idleTimeoutMs / 4)));
  const cleanupTimer = setInterval(() => {
    void cleanupIdleSessions();
  }, cleanupEveryMs);

  return { closed, close: shutdown };
}

function receiveRequest(socket: Socket, handle: (request: DaemonRequest) => void): void {
  let buffer = "";
  let handled = false;
  socket.setEncoding("utf8");
  socket.on("data", (chunk: string) => {
    if (handled) return;
    buffer += chunk;
    if (Buffer.byteLength(buffer) > MAX_REQUEST_BYTES) {
      handled = true;
      socket.end(`${JSON.stringify(errorResponse("", new Error("request exceeded 1 MiB")))}\n`);
      return;
    }

    const newline = buffer.indexOf("\n");
    if (newline < 0) return;
    handled = true;
    let payload: unknown;
    try {
      payload = JSON.parse(buffer.slice(0, newline));
    } catch {
      socket.end(`${JSON.stringify(errorResponse("", new Error("invalid daemon JSON")))}\n`);
      return;
    }
    const parsed = daemonRequestSchema.safeParse(payload);
    if (!parsed.success) {
      socket.end(`${JSON.stringify(errorResponse("", new Error("invalid daemon request")))}\n`);
      return;
    }
    handle(parsed.data);
  });
  socket.once("error", () => undefined);
}

function listen(server: Server, endpoint: string): Promise<void> {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(endpoint, () => {
      server.off("error", reject);
      resolve();
    });
  });
}

function closeServer(server: Server): Promise<void> {
  return new Promise((resolve, reject) => {
    if (!server.listening) {
      resolve();
      return;
    }
    server.close((error) => {
      if (error) reject(error);
      else resolve();
    });
  });
}

function endpointIsActive(endpoint: string): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = createConnection(endpoint);
    socket.once("connect", () => {
      socket.end();
      resolve(true);
    });
    socket.once("error", () => resolve(false));
  });
}
