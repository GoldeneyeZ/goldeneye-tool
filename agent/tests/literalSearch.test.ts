import { describe, expect, it, vi } from "vitest";
import { CodebaseMemoryMcpClient } from "../src/adapters/codebaseMemoryMcp/CodebaseMemoryMcpClient.js";

function searchResponse(...qualifiedNames: string[]) {
  return {
    results: qualifiedNames.map((qualifiedName, index) => ({
      qualified_name: qualifiedName,
      label: "Class",
      file_path: `src/${qualifiedName}.java`,
      start_line: index + 1,
      signature: `class ${qualifiedName}`,
    })),
  };
}

describe("Goldeneye literal-safe search regressions", () => {
  it("splits pipe alternatives and applies stable merge, dedupe, and one global limit", async () => {
    const invoke = vi.fn(async (_toolName: string, args: Record<string, unknown>) => {
      if (args.query === '"SecurityConfig"') {
        return searchResponse("SecurityConfig", "SharedConfig", "ExtraSecurityConfig");
      }
      if (args.query === '"JwtAuthenticationFilter"') {
        return searchResponse(
          "JwtAuthenticationFilter",
          "SharedConfig",
          "ExtraAuthenticationFilter",
        );
      }
      throw new Error(`unsafe query forwarded: ${String(args.query)}`);
    });
    const client = new CodebaseMemoryMcpClient("abyssal-zenith", { invoke });

    const rows = await client.search("SecurityConfig|JwtAuthenticationFilter", { limit: 3 });

    expect(rows.map((row) => row.qualifiedName)).toEqual([
      "SecurityConfig",
      "JwtAuthenticationFilter",
      "SharedConfig",
    ]);
    expect(invoke.mock.calls).toEqual([
      [
        "search_graph",
        {
          project: "abyssal-zenith",
          query: '"SecurityConfig"',
          limit: 3,
        },
      ],
      [
        "search_graph",
        {
          project: "abyssal-zenith",
          query: '"JwtAuthenticationFilter"',
          limit: 3,
        },
      ],
    ]);
  });

  it("normalizes a Java annotation query before invoking Goldeneye FTS", async () => {
    const invoke = vi.fn(async () => searchResponse("ApplicationIntegrationTest"));
    const client = new CodebaseMemoryMcpClient("abyssal-zenith", { invoke });

    await expect(client.search("@SpringBootTest", { limit: 5 })).resolves.toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("search_graph", {
      project: "abyssal-zenith",
      query: '"SpringBootTest"',
      limit: 5,
    });
  });

  it("routes a leading wildcard to an escaped suffix name pattern", async () => {
    const invoke = vi.fn(async () => searchResponse("BookingServiceTest"));
    const client = new CodebaseMemoryMcpClient("abyssal-zenith", { invoke });

    await expect(client.search("*Test", { limit: 5 })).resolves.toHaveLength(1);
    expect(invoke).toHaveBeenCalledWith("search_graph", {
      project: "abyssal-zenith",
      name_pattern: ".*Test$",
      limit: 5,
    });
  });

  it("does not leak backend FTS grammar errors", async () => {
    const invoke = vi.fn(async () => {
      throw new Error("SQL logic error: fts5: syntax error near '|'");
    });
    const client = new CodebaseMemoryMcpClient("abyssal-zenith", { invoke });

    await expect(client.search("SecurityConfig", { limit: 5 })).rejects.toThrow(
      "GCAL search backend rejected a literal-safe query",
    );
  });
});
