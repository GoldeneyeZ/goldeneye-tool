import { GcalBackendError } from "../../domain/GcalBackendClient.js";

export interface GatewayInvokeInput {
  mcpUrl: string;
  toolId: string;
  args: Record<string, unknown>;
  fetch: typeof globalThis.fetch;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseJsonString(value: string): unknown {
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return value;
  }
}

export function unwrapMcpPayload(value: unknown): unknown {
  if (typeof value !== "string") {
    if (!isRecord(value)) {
      return value;
    }

    if (value.isError === true) {
      const structured = isRecord(value.structuredContent)
        ? value.structuredContent
        : undefined;
      const payload = unwrapMcpPayloadContent(value);
      const message =
        structured !== undefined &&
        typeof structured.message === "string" &&
        structured.message.trim() !== ""
          ? structured.message
          : payloadMessage(payload);
      throw new GcalBackendError(
        `MCP tool error: ${message}`,
        structured !== undefined && typeof structured.code === "string"
          ? structured.code
          : undefined,
        structured?.details,
      );
    }

    return unwrapMcpPayloadContent(value);
  }

  const parsed = parseJsonString(value);
  if (parsed === value) {
    return value;
  }

  return unwrapMcpPayload(parsed);
}

function unwrapMcpPayloadContent(value: Record<string, unknown>): unknown {
  const content = value.content;
  if (!Array.isArray(content) || content.length === 0) {
    return value;
  }

  const first = content[0];
  if (isRecord(first) && "text" in first) {
    return unwrapMcpPayload(first.text);
  }

  return unwrapMcpPayload(first);
}

function errorMessage(error: unknown): string {
  if (isRecord(error) && typeof error.message === "string" && error.message.trim() !== "") {
    return error.message;
  }

  if (isRecord(error) && typeof error.code === "number") {
    return `code ${error.code}`;
  }

  return "unknown error";
}

function payloadMessage(payload: unknown): string {
  if (typeof payload === "string" && payload.trim() !== "") {
    return payload;
  }

  if (payload === undefined || payload === null) {
    return "unknown error";
  }

  return JSON.stringify(payload);
}

export async function gatewayInvoke(input: GatewayInvokeInput): Promise<unknown> {
  const response = await input.fetch(input.mcpUrl, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: {
        name: "gateway.invoke",
        arguments: {
          id: input.toolId,
          args: input.args,
        },
      },
    }),
  });

  if (!response.ok) {
    throw new Error(`MCP request failed with HTTP ${response.status}`);
  }

  const json: unknown = await response.json();
  if (!isRecord(json)) {
    throw new Error("MCP response was not a JSON object");
  }

  if ("error" in json) {
    throw new Error(`MCP error: ${errorMessage(json.error)}`);
  }

  if (!("result" in json)) {
    throw new Error("MCP response did not include a result");
  }

  return unwrapMcpPayload(json.result);
}
