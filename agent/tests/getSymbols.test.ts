import { describe, expect, it, vi } from "vitest";
import type { GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import type { SelectedSymbol } from "../src/domain/types.js";
import {
  BATCH_SNIPPET_CHUNK_BYTES,
  getSymbols,
  MAX_BATCH_GET_SYMBOLS,
  validateBatchQualifiedNames,
} from "../src/workflows/getSymbols.js";

function selected(qualifiedName: string): SelectedSymbol {
  return {
    qualifiedName,
    kind: "Method",
    filePath: "src/Example.ts",
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

describe("getSymbols", () => {
  it("fetches sequentially and preserves input order across isolated failures", async () => {
    const calls: string[] = [];
    const get = vi.fn(async (qualifiedName: string) => {
      calls.push(qualifiedName);
      if (qualifiedName === "two") {
        throw new Error("missing\nsymbol");
      }
      return selected(qualifiedName);
    });
    const client = { get } as unknown as GcalBackendClient;

    const outcomes = await getSymbols(client, ["one", "two", "three"]);

    expect(calls).toEqual(["one", "two", "three"]);
    expect(outcomes).toEqual([
      { status: "ok", qualifiedName: "one", selected: selected("one") },
      { status: "error", qualifiedName: "two", message: "missing symbol" },
      { status: "ok", qualifiedName: "three", selected: selected("three") },
    ]);
  });

  it("rejects batches larger than 32 before fetching", () => {
    const names = Array.from({ length: MAX_BATCH_GET_SYMBOLS + 1 }, (_, index) => `id-${index}`);
    expect(() => validateBatchQualifiedNames(names)).toThrow(
      "gcal get accepts at most 32 symbols per batch",
    );
  });

  it("uses one direct 8192-byte chunk call per Goldeneye batch item", async () => {
    const get = vi.fn();
    const getSnippetChunk = vi.fn(async (qualifiedName: string) => selected(qualifiedName));
    const client = { get, getSnippetChunk } as unknown as GcalBackendClient;

    const outcomes = await getSymbols(client, ["one", "two"]);

    expect(get).not.toHaveBeenCalled();
    expect(getSnippetChunk.mock.calls).toEqual([
      ["one", { chunk: 1, chunkBytes: BATCH_SNIPPET_CHUNK_BYTES }],
      ["two", { chunk: 1, chunkBytes: BATCH_SNIPPET_CHUNK_BYTES }],
    ]);
    expect(outcomes.every((outcome) => outcome.status === "ok")).toBe(true);
  });
});
