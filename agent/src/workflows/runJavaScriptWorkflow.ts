import { Worker } from "node:worker_threads";
import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import type { SearchOptions, TraceOptions } from "../domain/types.js";
import { validateSearchQueries } from "./searchSymbols.js";

export const DEFAULT_JS_WORKFLOW_MAX_CALLS = 32;
export const MAX_JS_WORKFLOW_CALLS = 128;
export const DEFAULT_JS_WORKFLOW_TIMEOUT_MS = 30_000;
export const MAX_JS_WORKFLOW_TIMEOUT_MS = 120_000;
export const MAX_JS_WORKFLOW_CODE_BYTES = 64 * 1024;
export const MAX_JS_WORKFLOW_OUTPUT_BYTES = 48 * 1024;
export const MAX_JS_WORKFLOW_LOG_BYTES = 8 * 1024;
export const MAX_JS_WORKFLOW_SEARCH_LIMIT = 20;
export const MAX_JS_WORKFLOW_TRACE_LIMIT = 50;
export const MAX_JS_WORKFLOW_DEPTH = 4;
export const JS_WORKFLOW_SOURCE_CHUNK_BYTES = 8_192;

export interface JavaScriptWorkflowOptions {
  maxCalls: number;
  timeoutMs: number;
}

export interface JavaScriptWorkflowResult {
  value: unknown;
  callCount: number;
  stdout: string;
  stderr: string;
  logsTruncated: boolean;
}

interface WorkerCallMessage {
  type: "call";
  id: number;
  method: string;
  args: unknown[];
}

interface WorkerDoneMessage {
  type: "done";
  value: unknown;
}

interface WorkerErrorMessage {
  type: "error";
  message: string;
}

type WorkerMessage = WorkerCallMessage | WorkerDoneMessage | WorkerErrorMessage;

const WORKER_SOURCE = String.raw`
const { parentPort, workerData } = require("node:worker_threads");
const pending = new Map();
let nextId = 1;

function call(method, args) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    parentPort.postMessage({ type: "call", id, method, args });
  });
}

parentPort.on("message", (message) => {
  if (message.type !== "response") return;
  const request = pending.get(message.id);
  if (request === undefined) return;
  pending.delete(message.id);
  if (message.ok) request.resolve(message.value);
  else request.reject(new Error(message.error));
});

const gcal = Object.freeze({
  search(query, options = {}) {
    return call("search", [query, options]);
  },
  source(qualifiedName) {
    return call("source", [qualifiedName]);
  },
  async trySource(qualifiedName) {
    try {
      const value = await call("source", [qualifiedName]);
      if (value !== null && typeof value === "object" && !Array.isArray(value)) {
        return { ok: true, ...value };
      }
      return { ok: true, value };
    } catch (error) {
      return { ok: false, error: error instanceof Error ? error.message : String(error) };
    }
  },
  get(qualifiedName) {
    return call("source", [qualifiedName]);
  },
  callers(qualifiedName, options = {}) {
    return call("callers", [qualifiedName, options]);
  },
  async tryCallers(qualifiedName, options = {}) {
    try {
      return { ok: true, edges: await call("callers", [qualifiedName, options]) };
    } catch (error) {
      return { ok: false, error: error instanceof Error ? error.message : String(error) };
    }
  },
  callees(qualifiedName, options = {}) {
    return call("callees", [qualifiedName, options]);
  },
  async tryCallees(qualifiedName, options = {}) {
    try {
      return { ok: true, edges: await call("callees", [qualifiedName, options]) };
    } catch (error) {
      return { ok: false, error: error instanceof Error ? error.message : String(error) };
    }
  },
  select(rows, index = 0) {
    if (!Array.isArray(rows)) throw new Error("gcal.select rows must be an array");
    if (!Number.isSafeInteger(index) || index < 0) {
      throw new Error("gcal.select index must be a non-negative integer");
    }
    return rows[index] ?? null;
  },
});

(async () => {
  try {
    const AsyncFunction = Object.getPrototypeOf(async function () {}).constructor;
    const execute = new AsyncFunction("gcal", '"use strict";\n' + workerData.code);
    const value = await execute(gcal);
    parentPort.postMessage({ type: "done", value });
  } catch (error) {
    const message = error instanceof Error ? error.stack ?? error.message : String(error);
    parentPort.postMessage({ type: "error", message });
  }
})();
`;

export async function runJavaScriptWorkflow(
  client: GcalBackendClient,
  code: string,
  options: JavaScriptWorkflowOptions,
): Promise<JavaScriptWorkflowResult> {
  validateJavaScriptWorkflow(code, options);

  return new Promise((resolve, reject) => {
    const worker = new Worker(WORKER_SOURCE, {
      eval: true,
      workerData: { code },
      stdout: true,
      stderr: true,
    });
    let settled = false;
    let callCount = 0;
    let stdout = "";
    let stderr = "";
    let logsTruncated = false;
    const timeout = setTimeout(() => {
      fail(new Error(`gcal workflow JavaScript timed out after ${options.timeoutMs} ms`));
    }, options.timeoutMs);

    worker.stdout?.on("data", (chunk: Buffer) => {
      const captured = appendBounded(stdout, chunk.toString("utf8"), MAX_JS_WORKFLOW_LOG_BYTES);
      stdout = captured.value;
      logsTruncated ||= captured.truncated;
    });
    worker.stderr?.on("data", (chunk: Buffer) => {
      const captured = appendBounded(stderr, chunk.toString("utf8"), MAX_JS_WORKFLOW_LOG_BYTES);
      stderr = captured.value;
      logsTruncated ||= captured.truncated;
    });
    worker.on("error", fail);
    worker.on("exit", (code) => {
      if (!settled) {
        fail(new Error(`gcal workflow JavaScript worker exited before returning (code ${code})`));
      }
    });
    worker.on("message", (message: unknown) => {
      if (!isWorkerMessage(message) || settled) {
        return;
      }
      if (message.type === "done") {
        finish(message.value);
        return;
      }
      if (message.type === "error") {
        fail(new Error(`gcal workflow JavaScript failed: ${firstLine(message.message)}`));
        return;
      }

      callCount += 1;
      if (callCount > options.maxCalls) {
        fail(new Error(`gcal workflow JavaScript exceeded ${options.maxCalls} backend calls`));
        return;
      }
      executeCall(client, message.method, message.args).then(
        (value) => postResponse(worker, message.id, true, value),
        (error: unknown) => postResponse(worker, message.id, false, singleLineError(error)),
      );
    });

    function finish(value: unknown): void {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      void worker.terminate();
      resolve({ value, callCount, stdout, stderr, logsTruncated });
    }

    function fail(error: Error): void {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      void worker.terminate();
      reject(error);
    }
  });
}

export function formatJavaScriptWorkflowValue(value: unknown): string {
  let rendered: string | undefined;
  try {
    rendered = typeof value === "string" ? value : JSON.stringify(value);
  } catch {
    throw new Error("gcal workflow JavaScript must return a serializable value");
  }
  if (rendered === undefined) {
    throw new Error("gcal workflow JavaScript must return a serializable value");
  }
  if (Buffer.byteLength(rendered, "utf8") > MAX_JS_WORKFLOW_OUTPUT_BYTES) {
    throw new Error(
      `gcal workflow JavaScript output exceeds ${MAX_JS_WORKFLOW_OUTPUT_BYTES} UTF-8 bytes`,
    );
  }
  return rendered;
}

function validateJavaScriptWorkflow(code: string, options: JavaScriptWorkflowOptions): void {
  if (code.trim().length === 0) {
    throw new Error("gcal workflow JavaScript must not be empty");
  }
  if (Buffer.byteLength(code, "utf8") > MAX_JS_WORKFLOW_CODE_BYTES) {
    throw new Error(`gcal workflow JavaScript exceeds ${MAX_JS_WORKFLOW_CODE_BYTES} UTF-8 bytes`);
  }
  if (options.maxCalls < 1 || options.maxCalls > MAX_JS_WORKFLOW_CALLS) {
    throw new Error(`gcal workflow --max-calls must be between 1 and ${MAX_JS_WORKFLOW_CALLS}`);
  }
  if (options.timeoutMs < 1 || options.timeoutMs > MAX_JS_WORKFLOW_TIMEOUT_MS) {
    throw new Error(
      `gcal workflow --timeout-ms must be between 1 and ${MAX_JS_WORKFLOW_TIMEOUT_MS}`,
    );
  }
}

async function executeCall(
  client: GcalBackendClient,
  method: string,
  args: unknown[],
): Promise<unknown> {
  if (method === "search") {
    const query = stringArgument(args[0], "gcal.search query");
    validateSearchQueries([query]);
    const options = searchOptions(args[1]);
    return client.search(query, options);
  }

  const qualifiedName = stringArgument(args[0], `gcal.${method} qualifiedName`);
  validateSearchQueries([qualifiedName]);
  if (method === "source") {
    return client.getSnippetChunk === undefined
      ? client.get(qualifiedName)
      : client.getSnippetChunk(qualifiedName, {
          chunk: 1,
          chunkBytes: JS_WORKFLOW_SOURCE_CHUNK_BYTES,
        });
  }

  const options = traceOptions(args[1]);
  const trace =
    method === "callers"
      ? await client.callers(qualifiedName, { depth: options.depth })
      : method === "callees"
        ? await client.callees(qualifiedName, { depth: options.depth })
        : undefined;
  if (trace === undefined) {
    throw new Error(`unknown GCAL workflow method: ${method}`);
  }
  return trace.slice(0, options.limit);
}

function searchOptions(value: unknown): Partial<SearchOptions> {
  const input = recordArgument(value, "gcal.search options");
  const limit = boundedInteger(
    input.limit ?? 5,
    "gcal.search limit",
    1,
    MAX_JS_WORKFLOW_SEARCH_LIMIT,
  );
  return {
    limit,
    label: optionalString(input.label, "gcal.search label"),
    filePattern: optionalString(input.filePattern, "gcal.search filePattern"),
    qualifiedNamePattern: optionalString(
      input.qualifiedNamePattern,
      "gcal.search qualifiedNamePattern",
    ),
  };
}

function traceOptions(value: unknown): TraceOptions & { limit: number } {
  const input = recordArgument(value, "GCAL trace options");
  return {
    depth: boundedInteger(input.depth ?? 1, "GCAL trace depth", 1, MAX_JS_WORKFLOW_DEPTH),
    limit: boundedInteger(input.limit ?? 20, "GCAL trace limit", 1, MAX_JS_WORKFLOW_TRACE_LIMIT),
  };
}

function recordArgument(value: unknown, name: string): Record<string, unknown> {
  if (value === undefined) return {};
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${name} must be an object`);
  }
  return value as Record<string, unknown>;
}

function stringArgument(value: unknown, name: string): string {
  if (typeof value !== "string") throw new Error(`${name} must be a string`);
  return value;
}

function optionalString(value: unknown, name: string): string | undefined {
  if (value === undefined) return undefined;
  return stringArgument(value, name);
}

function boundedInteger(value: unknown, name: string, minimum: number, maximum: number): number {
  if (!Number.isSafeInteger(value) || (value as number) < minimum || (value as number) > maximum) {
    throw new Error(`${name} must be an integer between ${minimum} and ${maximum}`);
  }
  return value as number;
}

function isWorkerMessage(value: unknown): value is WorkerMessage {
  if (value === null || typeof value !== "object") return false;
  const message = value as Record<string, unknown>;
  if (message.type === "done") return "value" in message;
  if (message.type === "error") return typeof message.message === "string";
  return (
    message.type === "call" &&
    typeof message.id === "number" &&
    typeof message.method === "string" &&
    Array.isArray(message.args)
  );
}

function postResponse(worker: Worker, id: number, ok: boolean, valueOrError: unknown): void {
  try {
    worker.postMessage(
      ok
        ? { type: "response", id, ok: true, value: valueOrError }
        : { type: "response", id, ok: false, error: valueOrError },
    );
  } catch {
    // Worker may have completed or timed out while a backend call was in flight.
  }
}

function appendBounded(
  current: string,
  addition: string,
  maxBytes: number,
): { value: string; truncated: boolean } {
  const combined = `${current}${addition}`;
  if (Buffer.byteLength(combined, "utf8") <= maxBytes) {
    return { value: combined, truncated: false };
  }
  return {
    value: utf8Prefix(combined, maxBytes),
    truncated: true,
  };
}

function utf8Prefix(value: string, maxBytes: number): string {
  const bytes = Buffer.from(value, "utf8");
  let end = Math.min(bytes.length, maxBytes);
  while (end > 0 && (bytes[end] & 0xc0) === 0x80) end -= 1;
  return bytes.subarray(0, end).toString("utf8");
}

function firstLine(value: string): string {
  return value.split(/\r?\n/, 1)[0] || "unknown error";
}

function singleLineError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/\s+/g, " ").trim() || "unknown error";
}
