// @vitest-environment node
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, describe, expect, it, vi } from "vitest";
import { apiUrl, normalizeApiBasePath } from "../api/basePath";
import { HTTP_API_CONTRACT, MCP_TOOL_CONTRACT } from "../api/contract";
import { callTool } from "../api/rpc";

afterEach(() => {
  globalThis.__GOLDENEYE_UI_CONFIG__ = undefined;
  vi.unstubAllGlobals();
});

describe("configurable API base path", () => {
  it("normalizes one local path prefix", () => {
    expect(normalizeApiBasePath(undefined)).toBe("");
    expect(normalizeApiBasePath("/")).toBe("");
    expect(normalizeApiBasePath("gateway/goldeneye/")).toBe("/gateway/goldeneye");
    expect(apiUrl("/api/layout?project=a%20b", "/gateway/")).toBe(
      "/gateway/api/layout?project=a%20b",
    );
  });

  it("rejects origins, traversal, fragments, and unknown routes", () => {
    expect(() => normalizeApiBasePath("https://evil.example")).toThrow();
    expect(() => normalizeApiBasePath("/gateway/../admin")).toThrow();
    expect(() => normalizeApiBasePath("/gateway#fragment")).toThrow();
    expect(() => apiUrl("/internal/config", "")).toThrow();
  });

  it("prefixes the unchanged MCP JSON-RPC envelope", async () => {
    globalThis.__GOLDENEYE_UI_CONFIG__ = { apiBasePath: "/goldeneye" };
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => ({ result: { content: [{ text: '{"projects":[]}' }] } }),
    });
    vi.stubGlobal("fetch", fetchMock);

    await expect(callTool("list_projects")).resolves.toEqual({ projects: [] });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, request] = fetchMock.mock.calls[0]!;
    expect(url).toBe("/goldeneye/rpc");
    expect(JSON.parse(request.body)).toMatchObject({
      jsonrpc: "2.0",
      method: "tools/call",
      params: { name: "list_projects", arguments: {} },
    });
  });
});

describe("upstream HTTP contract", () => {
  it("retains every server route and MCP tool name", () => {
    expect(HTTP_API_CONTRACT).toEqual([
      { method: "POST", path: "/rpc" },
      { method: "GET", path: "/api/layout" },
      { method: "GET", path: "/api/repo-info" },
      { method: "POST", path: "/api/index" },
      { method: "GET", path: "/api/index-status" },
      { method: "GET", path: "/api/ui-config" },
      { method: "DELETE", path: "/api/project" },
      { method: "GET", path: "/api/browse" },
      { method: "GET", path: "/api/adr" },
      { method: "POST", path: "/api/adr" },
      { method: "GET", path: "/api/project-health" },
      { method: "GET", path: "/api/processes" },
      { method: "GET", path: "/api/logs" },
      { method: "POST", path: "/api/process-kill" },
    ]);
    expect(MCP_TOOL_CONTRACT).toEqual([
      "list_projects",
      "get_graph_schema",
      "get_code_snippet",
    ]);
  });

  it("routes every production fetch through apiUrl", () => {
    const source = fileURLToPath(new URL("../", import.meta.url));
    const pending = [source];
    const violations: string[] = [];
    while (pending.length) {
      const directory = pending.pop();
      if (!directory) break;
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const file = path.join(directory, entry.name);
        if (entry.isDirectory()) pending.push(file);
        else if (/\.(ts|tsx)$/.test(entry.name) && !entry.name.includes(".test.")) {
          const text = readFileSync(file, "utf8");
          if (/fetch\s*\(\s*["'`]\/(?:api|rpc)/.test(text)) violations.push(file);
        }
      }
    }
    expect(violations).toEqual([]);
  });
});
