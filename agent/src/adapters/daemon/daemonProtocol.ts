import { z } from "zod";
import { GcalBackendError, type GcalBackendClient } from "../../domain/GcalBackendClient.js";
import type { SearchOptions, SnippetChunkOptions, TraceOptions } from "../../domain/types.js";

export const DAEMON_PROTOCOL_VERSION = 1;

const baseRequestSchema = z.object({
  id: z.string().min(1),
  version: z.literal(DAEMON_PROTOCOL_VERSION),
  command: z.string().min(1),
  project: z.string(),
});

export const daemonRequestSchema = z.discriminatedUnion("method", [
  baseRequestSchema.extend({
    method: z.literal("search"),
    args: z.tuple([z.string(), z.record(z.unknown())]),
  }),
  baseRequestSchema.extend({
    method: z.literal("symbol"),
    args: z.tuple([z.string(), z.record(z.unknown())]),
  }),
  baseRequestSchema.extend({
    method: z.literal("get"),
    args: z.tuple([z.string()]),
  }),
  baseRequestSchema.extend({
    method: z.literal("getSnippetManifest"),
    args: z.tuple([z.string(), z.number().int().positive()]),
  }),
  baseRequestSchema.extend({
    method: z.literal("getSnippetChunk"),
    args: z.tuple([z.string(), z.record(z.unknown())]),
  }),
  baseRequestSchema.extend({
    method: z.literal("callers"),
    args: z.tuple([z.string(), z.record(z.unknown())]),
  }),
  baseRequestSchema.extend({
    method: z.literal("callees"),
    args: z.tuple([z.string(), z.record(z.unknown())]),
  }),
  baseRequestSchema.extend({
    method: z.enum(["arch", "status", "projects"]),
    args: z.tuple([]),
  }),
  baseRequestSchema.extend({
    method: z.literal("index"),
    args: z.tuple([z.string()]),
  }),
]);

export type DaemonRequest = z.infer<typeof daemonRequestSchema>;
export type DaemonMethod = DaemonRequest["method"];

export interface DaemonSuccessResponse {
  id: string;
  ok: true;
  result: unknown;
}

export interface DaemonErrorResponse {
  id: string;
  ok: false;
  error: {
    name: string;
    message: string;
    code?: string;
    details?: unknown;
  };
}

export type DaemonResponse = DaemonSuccessResponse | DaemonErrorResponse;

export async function invokeBackend(
  client: GcalBackendClient,
  method: DaemonMethod,
  args: unknown[],
): Promise<unknown> {
  switch (method) {
    case "search":
      return client.search(args[0] as string, args[1] as Partial<SearchOptions>);
    case "symbol":
      return client.symbol(args[0] as string, args[1] as Partial<SearchOptions>);
    case "get":
      return client.get(args[0] as string);
    case "getSnippetManifest":
      if (!client.getSnippetManifest) throw unsupported(method);
      return client.getSnippetManifest(args[0] as string, args[1] as number);
    case "getSnippetChunk":
      if (!client.getSnippetChunk) throw unsupported(method);
      return client.getSnippetChunk(args[0] as string, args[1] as SnippetChunkOptions);
    case "callers":
      return client.callers(args[0] as string, args[1] as TraceOptions);
    case "callees":
      return client.callees(args[0] as string, args[1] as TraceOptions);
    case "arch":
      return client.arch();
    case "status":
      return client.status();
    case "index":
      return client.index(args[0] as string);
    case "projects":
      return client.projects();
  }
}

export function errorResponse(id: string, error: unknown): DaemonErrorResponse {
  if (error instanceof GcalBackendError) {
    return {
      id,
      ok: false,
      error: {
        name: error.name,
        message: error.message,
        ...(error.code === undefined ? {} : { code: error.code }),
        ...(error.details === undefined ? {} : { details: error.details }),
      },
    };
  }

  return {
    id,
    ok: false,
    error: {
      name: error instanceof Error ? error.name : "Error",
      message: error instanceof Error ? error.message : String(error),
    },
  };
}

export function responseError(response: DaemonErrorResponse): Error {
  if (response.error.name === "GcalBackendError") {
    return new GcalBackendError(response.error.message, response.error.code, response.error.details);
  }

  const error = new Error(response.error.message);
  error.name = response.error.name;
  return error;
}

function unsupported(method: string): Error {
  return new Error(`GCAL daemon backend does not support ${method}`);
}
