import { Buffer } from "node:buffer";
import { describe, expect, it } from "vitest";
import type { SelectedSymbol } from "../src/domain/types.js";
import {
  BATCH_GET_MAX_OUTPUT_BYTES,
  BATCH_GET_MAX_SOURCE_BYTES,
  formatBatchSourcesText,
  formatHydratedSearchText,
  formatMultiHopWorkflowText,
  formatSourceText,
  HYDRATED_SEARCH_MAX_OUTPUT_BYTES,
  HYDRATED_SEARCH_MAX_SNIPPET_BYTES,
} from "../src/formatters/textFormatters.js";

function selected(qualifiedName: string, source: string): SelectedSymbol {
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
    source,
  };
}

describe("formatBatchSourcesText", () => {
  it("prints deterministic qualified-name blocks in input order", () => {
    expect(
      formatBatchSourcesText([selected("one", "source one"), selected("two", "source two")]),
    ).toBe("# one\nsource one\n\n# two\nsource two");
  });

  it("keeps one-chunk UTF-8 source exact and gives partial chunks bounded continuation metadata", () => {
    const exact = selected("exact", "λ".repeat(100));
    exact.sourceChunk = {
      chunk: 1,
      chunkCount: 1,
      chunkBytes: 8192,
      sourceBytes: 200,
      sourceLines: 1,
      sourceSha256: "a".repeat(64),
      indexedFileHash: "indexed",
      chunkStartByte: 0,
      chunkEndByte: 200,
      eof: true,
      truncated: false,
    };
    expect(formatSourceText(exact)).toBe(exact.source);

    const partial = selected("partial", "λ".repeat(5_000));
    partial.sourceChunk = {
      ...exact.sourceChunk,
      chunkCount: 3,
      sourceBytes: 30_000,
      sourceLines: 900,
      chunkEndByte: 8_192,
      eof: false,
      truncated: true,
    };
    const output = formatHydratedSearchText(
      [
        {
          qualifiedName: "partial",
          label: "Method",
          filePath: "src/partial.ts",
          line: 1,
          signature: "partial()",
        },
      ],
      [
        {
          candidate: {
            qualifiedName: "partial",
            label: "Method",
            filePath: "src/partial.ts",
            line: 1,
            signature: "partial()",
          },
          selected: partial,
        },
      ],
    );

    expect(Buffer.byteLength(output, "utf8")).toBeLessThanOrEqual(HYDRATED_SEARCH_MAX_OUTPUT_BYTES);
    expect(output).toContain("chunk 1/3");
    expect(output).toContain("--chunk 2 --expected-source-sha");
    expect(output).not.toContain("�");
  });

  it("caps each source and aggregate UTF-8 output without splitting characters", () => {
    const symbols = Array.from({ length: 32 }, (_, index) =>
      selected(
        index === 0 ? "λ".repeat(1_000) : `symbol-${index}`,
        "λ".repeat(BATCH_GET_MAX_SOURCE_BYTES),
      ),
    );

    const output = formatBatchSourcesText(symbols);

    expect(Buffer.byteLength(output, "utf8")).toBeLessThanOrEqual(BATCH_GET_MAX_OUTPUT_BYTES);
    expect(output.match(/\[truncated; run single-symbol gcal get for full source\]/g)).toHaveLength(
      32,
    );
    expect(output).not.toContain("�");
    expect(output).not.toContain(symbols[0].qualifiedName);
    expect(output).toContain("...");
    for (const symbol of symbols.slice(1)) {
      expect(output).toContain(`# ${symbol.qualifiedName}\n`);
    }
  });
});

describe("formatHydratedSearchText", () => {
  it("prints candidates then stable snippet blocks", () => {
    const first = selected("one", "source one");
    const second = selected("two", "source two");
    const candidates = [
      {
        qualifiedName: "one",
        label: "Method" as const,
        filePath: "src/one.ts",
        line: 1,
        signature: "one()",
      },
      {
        qualifiedName: "two",
        label: "Method" as const,
        filePath: "src/two.ts",
        line: 2,
        signature: "two()",
      },
    ];

    expect(
      formatHydratedSearchText(candidates, [
        { candidate: candidates[0], selected: first },
        { candidate: candidates[1], selected: second },
      ]),
    ).toBe(
      "one\tMethod\tsrc/one.ts\t1\tone()\n" +
        "two\tMethod\tsrc/two.ts\t2\ttwo()\n\n" +
        "# snippet\tone\nsource one\n\n" +
        "# snippet\ttwo\nsource two",
    );
  });

  it("caps Unicode candidate/snippet output and represents every hydration", () => {
    const candidates = Array.from({ length: 20 }, (_, index) => ({
      qualifiedName: `symbol-${index}`,
      label: "Method" as const,
      filePath: `src/${index}.ts`,
      line: index,
      signature: "λ".repeat(1_000),
    }));
    const snippets = candidates.slice(0, 5).map((row) => ({
      candidate: row,
      selected: selected(row.qualifiedName, "λ".repeat(HYDRATED_SEARCH_MAX_SNIPPET_BYTES)),
    }));

    const output = formatHydratedSearchText(candidates, snippets);

    expect(Buffer.byteLength(output, "utf8")).toBeLessThanOrEqual(HYDRATED_SEARCH_MAX_OUTPUT_BYTES);
    expect(output).toContain("[candidate rows truncated]");
    expect(output.match(/\[snippet truncated; run gcal get for full source\]/g)).toHaveLength(5);
    expect(output).not.toContain("�");
    for (const snippet of snippets) {
      expect(output).toContain(`# snippet\t${snippet.candidate.qualifiedName}\n`);
    }
  });
});

describe("formatMultiHopWorkflowText", () => {
  it("prints deterministic selected, source, inbound, and outbound sections", () => {
    const symbol = selected("example.Service.run", "run(): void {}");
    const trace = {
      sourceQualifiedName: "example.Caller.run",
      targetQualifiedName: symbol.qualifiedName,
      relatedQualifiedName: "example.Caller.run",
      hop: 1,
      filePath: "src/Caller.ts",
      line: 4,
    };

    expect(
      formatMultiHopWorkflowText({
        candidates: [],
        selectedQualifiedName: symbol.qualifiedName,
        source: symbol,
        inbound: [trace],
        inboundTotal: 1,
        outbound: [{ ...trace, relatedQualifiedName: "example.Dependency.run" }],
        outboundTotal: 1,
        failures: [],
      }),
    ).toBe(
      "# selected\nqualified_name=example.Service.run\n\n" +
        "# source\nrun(): void {}\n\n" +
        "# inbound\nexample.Caller.run\t1\tsrc/Caller.ts\t4\n\n" +
        "# outbound\nexample.Dependency.run\t1\tsrc/Caller.ts\t4",
    );
  });
});
