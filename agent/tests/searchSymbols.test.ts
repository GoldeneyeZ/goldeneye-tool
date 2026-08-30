import { describe, expect, it, vi } from "vitest";
import type { GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import type { SelectedSymbol, SymbolCandidate } from "../src/domain/types.js";
import {
  MAX_SEARCH_QUERIES,
  SEARCH_SNIPPET_CHUNK_BYTES,
  searchSymbols,
  validateSearchQueries,
} from "../src/workflows/searchSymbols.js";

function candidate(qualifiedName: string): SymbolCandidate {
  return {
    qualifiedName,
    label: "Method",
    filePath: `src/${qualifiedName}.ts`,
    line: 1,
    signature: `function ${qualifiedName}()`,
  };
}

function selected(qualifiedName: string): SelectedSymbol {
  return {
    qualifiedName,
    kind: "Method",
    filePath: `src/${qualifiedName}.ts`,
    startLine: 1,
    endLine: 1,
    lines: 1,
    complexity: null,
    cognitive: null,
    visibility: "",
    signature: "",
    returnType: "",
    decorators: "",
    callers: null,
    callees: null,
    source: `source:${qualifiedName}`,
  };
}

describe("searchSymbols", () => {
  it("deduplicates queries/results and keeps stable rank across partial failures", async () => {
    const search = vi.fn(async (query: string) => {
      if (query === "bad") {
        throw new Error("stale\nindex");
      }
      return query === "first"
        ? [candidate("one"), candidate("shared")]
        : [candidate("shared"), candidate("three")];
    });
    const get = vi.fn(async (qualifiedName: string) => {
      if (qualifiedName === "shared") {
        throw new Error("too large");
      }
      return selected(qualifiedName);
    });
    const client = { get, search } as unknown as GcalBackendClient;

    const result = await searchSymbols(
      client,
      ["first", "first", "bad", "third"],
      { limit: 20 },
      2,
    );

    expect(search.mock.calls.map(([query]) => query)).toEqual(["first", "bad", "third"]);
    expect(result.candidates.map(({ qualifiedName }) => qualifiedName)).toEqual([
      "one",
      "shared",
      "three",
    ]);
    expect(result.snippets.map(({ candidate: row }) => row.qualifiedName)).toEqual(["one"]);
    expect(result.queryFailures).toEqual([{ query: "bad", message: "stale index" }]);
    expect(result.hydrationFailures).toEqual([{ qualifiedName: "shared", message: "too large" }]);
  });

  it("enforces branch count, UTF-8 query size, and global candidate cap", async () => {
    expect(() =>
      validateSearchQueries(
        Array.from({ length: MAX_SEARCH_QUERIES + 1 }, (_, index) => `query-${index}`),
      ),
    ).toThrow("gcal search accepts at most 8 query branches");
    expect(() => validateSearchQueries(["λ".repeat(257)])).toThrow(
      "gcal search query branches must not exceed 512 UTF-8 bytes",
    );
    expect(() => validateSearchQueries(["λ".repeat(256)])).not.toThrow();
    expect(() => validateSearchQueries(["  "])).toThrow(
      "gcal search enhanced query branches must not be empty or whitespace",
    );

    const rows = Array.from({ length: 25 }, (_, index) => candidate(`symbol-${index}`));
    const search = vi.fn().mockResolvedValue(rows);
    const result = await searchSymbols(
      { search } as unknown as GcalBackendClient,
      ["query"],
      { limit: 100 },
      undefined,
    );

    expect(search).toHaveBeenCalledWith("query", { limit: 20 });
    expect(result.candidates).toHaveLength(20);
  });

  it("hydrates each Goldeneye snippet with one direct 4096-byte chunk call", async () => {
    const rows = [candidate("one"), candidate("two")];
    const search = vi.fn().mockResolvedValue(rows);
    const get = vi.fn();
    const getSnippetChunk = vi.fn(async (qualifiedName: string) => selected(qualifiedName));

    const result = await searchSymbols(
      { get, getSnippetChunk, search } as unknown as GcalBackendClient,
      ["query"],
      { limit: 5 },
      2,
    );

    expect(get).not.toHaveBeenCalled();
    expect(getSnippetChunk.mock.calls).toEqual([
      ["one", { chunk: 1, chunkBytes: SEARCH_SNIPPET_CHUNK_BYTES }],
      ["two", { chunk: 1, chunkBytes: SEARCH_SNIPPET_CHUNK_BYTES }],
    ]);
    expect(result.snippets).toHaveLength(2);
  });

  it.each([
    { limit: 1, expected: ["first-0"] },
    { limit: 5, expected: ["first-0", "second-0", "first-1", "second-1", "first-2"] },
    {
      limit: 20,
      expected: [
        "first-0",
        "second-0",
        "first-1",
        "second-1",
        "first-2",
        "first-3",
        "second-3",
        "first-4",
        "second-4",
        "first-5",
        "second-5",
        "first-6",
        "second-6",
        "first-7",
        "second-7",
        "first-8",
        "second-8",
        "first-9",
        "second-9",
        "first-10",
      ],
    },
  ])("merges branches rank-major at global limit $limit", async ({ expected, limit }) => {
    const firstRows = Array.from({ length: 20 }, (_, index) => candidate(`first-${index}`));
    const secondRows = Array.from({ length: 20 }, (_, index) => candidate(`second-${index}`));
    secondRows[2] = candidate("first-2");
    const search = vi.fn(async (query: string) => (query === "first" ? firstRows : secondRows));

    const result = await searchSymbols(
      { search } as unknown as GcalBackendClient,
      ["first", "second"],
      { limit },
      undefined,
    );

    expect(result.candidates.map(({ qualifiedName }) => qualifiedName)).toEqual(expected);
  });
});
