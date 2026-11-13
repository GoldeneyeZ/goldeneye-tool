import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import { similarNamePattern, type GraphNode, type ToolClient } from "../src/kernel/codeGraph.js";
import { displayName, isTestPath } from "../src/kernel/displayName.js";
import { parseLocator } from "../src/kernel/locator.js";
import { FreshnessProbe } from "../src/kernel/freshness.js";
import { locateDeclaration } from "../src/kernel/relocate.js";
import { SourceReader } from "../src/kernel/sourceFiles.js";
import {
  globPattern,
  globsPattern,
  grepCode,
  implementations,
  pathsPattern,
  references,
  sameLanguageFilter,
  searchCode,
  separatePaths,
  testsUsing,
  summarizeLayout,
} from "../src/workflows/exploreCode.js";
import {
  fairBudgets,
  leadingDocStart,
  readLocators,
  type CodeContext,
} from "../src/workflows/readCode.js";
import { resolveSymbol } from "../src/workflows/resolveSymbol.js";
import { parseSymbolLocator } from "../src/kernel/locator.js";

const PROJECT = "D-Dev-.work-shop";
const BILLING = "src/main/java/com/shop/Billing.java";
const LEDGER = "src/main/java/com/shop/Ledger.java";
const BILLING_TEST = "src/test/java/com/shop/BillingTests.java";

function qn(filePath: string, ...rest: string[]): string {
  const segments = filePath.replace(/\.java$/, "").split("/");
  return [PROJECT, ...segments, ...rest].join(".");
}

function node(
  filePath: string,
  label: string,
  rest: string[],
  startLine: number,
  endLine: number,
): GraphNode {
  const last = rest[rest.length - 1] ?? "";
  return {
    qualifiedName: qn(filePath, ...rest),
    name: last.replace(/#\d+$/, ""),
    label,
    filePath,
    startLine,
    endLine,
  };
}

const NODES: GraphNode[] = [
  node(BILLING, "Module", [], 1, 20),
  node(BILLING, "Class", ["Billing"], 3, 20),
  node(BILLING, "Method", ["Billing", "Billing"], 4, 4),
  node(BILLING, "Method", ["Billing", "charge"], 6, 8),
  node(BILLING, "Method", ["Billing", "charge#2"], 10, 12),
  node(BILLING, "Method", ["Billing", "refund"], 14, 16),
  node(LEDGER, "Interface", ["Ledger"], 3, 5),
  node(LEDGER, "Method", ["Ledger", "record"], 4, 4),
  node(BILLING_TEST, "Class", ["BillingTests"], 3, 9),
  node(BILLING_TEST, "Method", ["BillingTests", "refund"], 5, 8),
];

const BILLING_SOURCE = [
  "package com.shop;",
  "",
  "public class Billing implements Ledger {",
  "  public Billing() {}",
  "",
  "  public void charge(int cents) {",
  "    record(cents);",
  "  }",
  "",
  "  public void charge(int cents, String note) {",
  "    record(cents);",
  "  }",
  "",
  "  public void refund(int cents) {",
  "    record(-cents);",
  "  }",
  "",
  "  public void record(int cents) {}",
  "",
  "}",
].join("\n");

/** Qualified names embed the dotted file path; that is enough to map a symbol to its file. */
function staleKey(filePath: string): string {
  return filePath.replace(/\.java$/, "").split("/").join(".");
}

interface FakeGraphOptions {
  staleFiles?: string[];
  textResults?: Array<Record<string, unknown>>;
  /** Text searches run as a regular expression find nothing. */
  regexFindsNothing?: boolean;
  /** Receives the `regex` argument of every text search. */
  textSearchModes?: boolean[];
  subtypes?: Record<string, GraphNode[]>;
  /** Symbols indexed besides the Java fixture. */
  extraNodes?: GraphNode[];
  /** What the backend says matched before its limit. */
  textTotal?: number;
}

function fakeGraph(root: string, options: FakeGraphOptions = {}): ToolClient {
  const nodes = [...NODES, ...(options.extraNodes ?? [])];
  const quoted = (query: string) => /'([^']*)'/.exec(query)?.[1] ?? "";
  const row = (item: GraphNode) => [
    item.qualifiedName,
    item.name,
    [item.label],
    item.filePath,
    item.startLine,
    item.endLine,
  ];

  return {
    async tool(toolName, args) {
      if (toolName === "index_status") return { root_path: root };
      if (toolName === "get_code_snippet") {
        if (options.staleFiles?.some((path) => String(args.qualified_name).includes(staleKey(path)))) {
          throw new Error("MCP tool error: indexed source file is stale: " + String(args.qualified_name));
        }
        return { code: "" };
      }
      if (toolName === "search_code") {
        options.textSearchModes?.push(args.regex === true);
        const empty = options.regexFindsNothing === true && args.regex === true;
        return { results: empty ? [] : (options.textResults ?? []), total_results: options.textTotal ?? 0 };
      }
      if (toolName === "search_graph") {
        const pattern = args.name_pattern === undefined ? undefined : String(args.name_pattern);
        const name =
          pattern === undefined
            ? undefined
            : pattern.startsWith("(?i)")
              ? new RegExp(pattern.slice(4), "i")
              : new RegExp(pattern);
        const qualified = args.qn_pattern === undefined ? undefined : new RegExp(String(args.qn_pattern));
        return {
          results: nodes.filter(
            (item) =>
              (name === undefined || name.test(item.name)) &&
              (qualified === undefined || qualified.test(item.qualifiedName)),
          ).map((item) => ({
            qualified_name: item.qualifiedName,
            name: item.name,
            label: item.label,
            file_path: item.filePath,
            start_line: item.startLine,
            end_line: item.endLine,
          })),
        };
      }
      if (toolName === "query_graph") {
        const query = String(args.query);
        const value = quoted(query);
        if (query.includes("DEFINES")) {
          return {
            rows: nodes.filter(
              (item) =>
                item.qualifiedName.startsWith(`${value}.`) &&
                !item.qualifiedName.slice(value.length + 1).includes("."),
            ).map(row),
          };
        }
        if (query.includes("INHERITS")) {
          return { rows: (options.subtypes?.[value] ?? []).map((item) => [...row(item), "IMPLEMENTS"]) };
        }
        if (query.includes("(f:File)")) {
          const paths = [...new Set(nodes.map((item) => item.filePath))];
          return { rows: paths.filter((path) => path.endsWith(value)).map((path) => [path]) };
        }
        const line = /start_line <= (\d+)/.exec(query);
        return {
          rows: nodes.filter(
            (item) =>
              item.filePath === value &&
              (line === null ||
                (item.startLine <= Number(line[1]) && item.endLine >= Number(line[1]))),
          ).map(row),
        };
      }
      throw new Error(`unexpected tool ${toolName}`);
    },
  };
}

let workspace: string | undefined;

async function createContext(options: FakeGraphOptions = {}): Promise<CodeContext> {
  workspace = await mkdtemp(join(tmpdir(), "gcal-discovery-"));
  await mkdir(join(workspace, dirname(BILLING)), { recursive: true });
  await writeFile(join(workspace, BILLING), BILLING_SOURCE);
  const client = fakeGraph(workspace, options);
  return {
    client,
    project: PROJECT,
    root: workspace,
    reader: new SourceReader(workspace),
    freshness: new FreshnessProbe(client),
  };
}

afterEach(async () => {
  if (workspace !== undefined) await rm(workspace, { recursive: true, force: true });
  workspace = undefined;
});

describe("locators", () => {
  it("separates symbols, files, lines and ranges", () => {
    expect(parseLocator("com.shop.Billing.charge")).toMatchObject({
      kind: "symbol",
      segments: ["com", "shop", "Billing", "charge"],
      overload: undefined,
    });
    expect(parseLocator("Billing#charge(int)")).toMatchObject({ segments: ["Billing", "charge"] });
    expect(parseLocator("Billing::charge#2")).toMatchObject({ segments: ["Billing", "charge"], overload: 2 });
    expect(parseLocator("src\\main\\Billing.java")).toMatchObject({ kind: "file", path: "src/main/Billing.java" });
    expect(parseLocator("Billing.java:42")).toMatchObject({ kind: "file", startLine: 42, endLine: undefined });
    expect(parseLocator("Billing.java:10-60")).toMatchObject({ kind: "file", startLine: 10, endLine: 60 });
  });

  it("shortens backend identifiers to what follows the file segment", () => {
    expect(displayName(NODES[3], PROJECT)).toBe("Billing.charge");
    expect(displayName(NODES[4], PROJECT)).toBe("Billing.charge#2");
    expect(displayName(NODES[0], PROJECT)).toBe("Billing");
    expect(isTestPath(BILLING_TEST)).toBe(true);
    expect(isTestPath(BILLING)).toBe(false);
  });
});

describe("symbol resolution", () => {
  it("resolves a package-qualified name and returns every overload", async () => {
    const context = await createContext();
    const resolution = await resolveSymbol(
      context.client,
      PROJECT,
      parseSymbolLocator("com.shop.Billing.charge"),
    );

    expect(resolution.kind).toBe("resolved");
    expect(resolution.kind === "resolved" && resolution.nodes.map((item) => item.startLine)).toEqual([6, 10]);
  });

  it("picks one overload, the type over its constructor, and production over tests", async () => {
    const context = await createContext();
    const resolve = (raw: string) => resolveSymbol(context.client, PROJECT, parseSymbolLocator(raw));

    expect(await resolve("Billing.charge#2")).toMatchObject({ nodes: [{ startLine: 10 }] });
    expect(await resolve("Billing")).toMatchObject({ kind: "resolved", nodes: [{ label: "Class" }] });
    expect(await resolve("refund")).toMatchObject({ kind: "resolved", nodes: [{ filePath: BILLING }] });
    expect(await resolve("wrong.pkg.Billing.refund")).toMatchObject({ kind: "resolved" });
    expect(await resolve("Billing.missing")).toMatchObject({ kind: "not-found" });
  });
});

describe("re-locating declarations in edited files", () => {
  it("tells declarations from calls and picks the nearest or the nth overload", () => {
    const java = BILLING_SOURCE.split("\n");

    expect(locateDeclaration(java, "charge", 6)).toEqual({ startLine: 6, endLine: 8 });
    expect(locateDeclaration(java, "charge", 11)).toEqual({ startLine: 10, endLine: 12 });
    expect(locateDeclaration(java, "charge", 1, { occurrence: 2 })).toEqual({ startLine: 10, endLine: 12 });
    // Lines 7, 11 and 15 call record(...); only line 18 declares it.
    expect(locateDeclaration(java, "record", 7)).toEqual({ startLine: 18, endLine: 18 });
    expect(locateDeclaration(java, "Billing", 9, { type: true })).toEqual({ startLine: 3, endLine: 20 });
    expect(locateDeclaration(java, "Billing", 9)).toEqual({ startLine: 4, endLine: 4 });
    expect(locateDeclaration(java, "missing", 5)).toBeUndefined();
  });

  it("handles attributes, braces inside literals, and indentation bodies", () => {
    const rust = [
      "#[must_use]",
      "pub fn label(kind: &str) -> String {",
      '    let open = "{";',
      "    format!(\"{open}{kind}\")",
      "}",
      "fn other() { label(\"x\"); }",
    ];
    const python = ["class Job:", "    def run(self):", "        self.step()", "", "    def step(self):", "        pass"];

    expect(locateDeclaration(rust, "label", 4)).toEqual({ startLine: 1, endLine: 5 });
    expect(locateDeclaration(python, "run", 2)).toEqual({ startLine: 2, endLine: 3 });
    expect(locateDeclaration(python, "Job", 1, { type: true })).toEqual({ startLine: 1, endLine: 6 });
  });
});

describe("reading code", () => {
  it("follows a symbol to its new lines when the file changed after indexing", async () => {
    const context = await createContext({ staleFiles: [BILLING] });
    await writeFile(join(context.root, BILLING), `// moved\n// down\n// by three\n${BILLING_SOURCE}`);

    const result = await readLocators(context, ["Billing.refund"]);

    expect(result.text).toBe(
      [
        `# Billing.refund Method ${BILLING}:17-19`,
        "# current: these are the lines of the working tree (the file changed since indexing)",
        "  public void refund(int cents) {",
        "    record(-cents);",
        "  }",
      ].join("\n"),
    );
  });

  it("prints a small file whole when its symbols are read to be changed", async () => {
    const context = await createContext();
    const source = BILLING_SOURCE.replace(/\n$/, "").split("\n");

    const forEdit = await readLocators(context, ["Billing.refund", "Billing.charge"], { forEdit: true });
    const [header, ...body] = forEdit.text.split("\n");
    expect(header).toMatch(new RegExp(`^# ${BILLING}:1-${source.length} whole \\(small file\\); refund \\d+, charge \\d+`));
    // Once, however many of its symbols were named.
    expect(body).toEqual(source);

    // A type in a small file comes with the file's package and imports too.
    const type = await readLocators(context, ["Billing"], { forEdit: true });
    expect(type.text.split("\n").slice(1)).toEqual(source);

    // Without the intent to edit, a symbol is its own lines.
    const plain = await readLocators(context, ["Billing.refund"]);
    expect(plain.text.startsWith("# Billing.refund Method")).toBe(true);
  });

  it("prints overloads with their location and exact working-tree lines", async () => {
    const context = await createContext();
    const result = await readLocators(context, ["com.shop.Billing.charge"]);

    expect(result.ok).toBe(true);
    expect(result.text).toContain(`# Billing.charge Method ${BILLING}:6-8\n  public void charge(int cents) {`);
    expect(result.text).toContain(`# Billing.charge#2 Method ${BILLING}:10-12`);
    expect(result.text).not.toContain(PROJECT);
  });

  it("reads the symbol at a line, a raw range, and reports misses", async () => {
    const context = await createContext();

    expect((await readLocators(context, [`${BILLING}:15`])).text).toContain("# Billing.refund Method");
    expect((await readLocators(context, ["Billing.java:6-7"])).text).toBe(
      `# ${BILLING}:6-7 of 20 lines\n  public void charge(int cents) {\n    record(cents);`,
    );

    // One miss in a batch is reported in the text and does not fail the command.
    const missing = await readLocators(context, ["Billing.refund", "Nope.nothing"]);
    expect(missing.ok).toBe(true);
    expect(missing.text).toContain("# Billing.refund Method");
    expect(missing.text).toContain("# not found: Nope.nothing");
    expect((await readLocators(context, ["Nope.nothing", "Nope.other"])).ok).toBe(false);
  });

  it("prefers a C definition to its prototype and forward declarations", async () => {
    const c = (label: string, filePath: string, rest: string, startLine: number, endLine: number): GraphNode => ({
      qualifiedName: `${PROJECT}.${filePath.replace(/\.[ch]$/, "").split("/").join(".")}.${rest}`,
      name: rest.split(".").pop() ?? rest,
      label,
      filePath,
      startLine,
      endLine,
    });
    const context = await createContext({
      extraNodes: [
        c("Function", "src/pass.c", "run_pass", 10, 40),
        c("Function", "src/internal.h", "run_pass", 5, 6),
        c("Class", "src/mcp.h", "pipeline", 140, 140),
        c("Class", "src/pipeline.c", "pipeline", 75, 105),
        c("Class", "src/pipeline.h", "pipeline_t.pipeline", 27, 30),
        c("Type", "src/graph.h", "gbuf_t", 18, 18),
        c("Type", "src/pipeline.h", "gbuf_t", 23, 23),
        c("Class", "src/route.c", "route_ctx_t.gbuf_t", 159, 162),
      ],
    });
    const locate = (raw: string) => resolveSymbol(context.client, PROJECT, parseSymbolLocator(raw));

    expect(await locate("run_pass")).toMatchObject({
      kind: "resolved",
      nodes: [{ filePath: "src/pass.c" }],
      declarations: [{ filePath: "src/internal.h" }],
    });
    expect((await readLocators(context, ["run_pass"])).text).toContain("# also declared: src/internal.h:5-6");
    expect(await locate("pipeline")).toMatchObject({
      kind: "resolved",
      nodes: [{ filePath: "src/pipeline.c", startLine: 75 }],
    });
    // Two typedefs of one name: neither is a definition, both are listed.
    const typedefs = await locate("gbuf_t");
    expect(typedefs.kind).toBe("ambiguous");
    expect(typedefs.kind === "ambiguous" && typedefs.nodes.map((item) => item.filePath)).toEqual([
      "src/graph.h",
      "src/pipeline.h",
    ]);
  });

  it("answers a member asked on the wrong owner, or not indexed, without a second call", async () => {
    const context = await createContext({
      textResults: [{ file: "src/limits.h", label: "", match_lines: [2, 5] }],
    });
    await mkdir(join(context.root, "src"), { recursive: true });
    await writeFile(
      join(context.root, "src/limits.h"),
      ["// limits", "#define MAX_RETRIES \\", "  3", "", "  schedule(MAX_RETRIES, retry_budget);"].join("\n"),
    );
    context.reader = new SourceReader(context.root);

    // `refund` exists once outside tests, on Billing.
    const elsewhere = await readLocators(context, ["Ledger.refund"]);
    expect(elsewhere.ok).toBe(true);
    expect(elsewhere.text).toContain("# not found: Ledger.refund; showing the only symbol named refund, Billing.refund");
    expect(elsewhere.text).toContain("    record(-cents);");

    // `Billing.record` is in the file and not in the index.
    const unindexed = await readLocators(context, ["Billing.record"]);
    expect(unindexed.text).toBe(
      [
        `# Billing.record ${BILLING}:18-18`,
        "# note: not in the index; declaration located by text",
        "  public void record(int cents) {}",
      ].join("\n"),
    );

    // A macro is no symbol; its text is found.
    const macro = await readLocators(context, ["MAX_RETRIES"]);
    expect(macro.text).toBe(
      [
        "# MAX_RETRIES src/limits.h:1-3",
        "# note: not in the index; declaration located by text",
        "// limits",
        "#define MAX_RETRIES \\",
        "  3",
      ].join("\n"),
    );

    // No declaration anywhere: the lines that mention the name.
    const mentions = await readLocators(context, ["retry_budget"]);
    expect(mentions.text).toContain("# not found: retry_budget; no symbol with that name is indexed. Text mentions");
    expect(mentions.text).toContain("src/limits.h:5: schedule(MAX_RETRIES, retry_budget);");
  });

  it("returns a type whole by default and as an outline on request", async () => {
    const context = await createContext();

    const whole = await readLocators(context, ["Billing"]);
    expect(whole.text).toContain("# Billing Class");
    expect(whole.text).toContain("    record(-cents);");

    const outline = await readLocators(context, ["Billing"], { detail: "outline" });
    expect(outline.text).toContain("# outline Billing Class");
    expect(outline.text).not.toContain("record(-cents)");
    expect(outline.text).toContain("whole: gcal get Billing --full");

    const fileOutline = await readLocators(context, ["Billing.java"], { detail: "outline" });
    expect(fileOutline.text).toContain(`# outline ${BILLING} lines=20`);
  });

  it("starts a symbol at the doc comment and annotations above it", () => {
    const java = ["class A {", "", "  /**", "   * Cuts text.", "   */", "  @Deprecated", "  void cut() {}", "  void other() {}", "}"];
    expect(leadingDocStart(java, 7)).toBe(3);
    expect(leadingDocStart(java, 8)).toBe(8);

    const rust = ["use std::fmt;", "/// Cuts text.", "#[inline]", "fn cut() {}"];
    expect(leadingDocStart(rust, 4)).toBe(2);

    const python = ["x = 1", "# Cuts text.", "@cache", "def cut(): pass"];
    expect(leadingDocStart(python, 4)).toBe(2);
  });

  it("finds a Go method by its receiver", async () => {
    const providers = "internal/parser/providers.go";
    const context = await createContext({
      extraNodes: [node(providers, "Method", ["Parse"], 3, 3), node(providers, "Method", ["Parse#2"], 5, 5)],
    });
    await mkdir(join(workspace!, "internal/parser"), { recursive: true });
    await writeFile(
      join(workspace!, providers),
      "package parser\n\nfunc (p *claudeProvider) Parse(path string) error { return nil }\n\nfunc (p codexProvider) Parse(path string) error { return p.read(path) }\n",
    );

    const exact = await readLocators(context, ["parser.codexProvider.Parse"]);
    expect(exact.text).toContain("return p.read(path)");
    expect(exact.text).not.toContain("claudeProvider");

    // Written with another case than the receiver has.
    const cased = await readLocators(context, ["ClaudeProvider.Parse"]);
    expect(cased.text).toContain("func (p *claudeProvider) Parse(path string) error { return nil }");
    expect(cased.text).not.toContain("# not found");
    expect(cased.text).not.toContain("codexProvider");
  });

  it("follows a forward typedef to the struct it names", async () => {
    const header = "src/store/store.h";
    const other = "src/pipeline/pipeline.h";
    const source = "src/store/store.c";
    const context = await createContext({
      extraNodes: [
        node(header, "Type", ["cbm_store_t"], 3, 3),
        node(other, "Type", ["cbm_store_t"], 1, 1),
        node(source, "Class", ["cbm_store"], 2, 5),
      ],
    });
    await mkdir(join(workspace!, "src/store"), { recursive: true });
    await mkdir(join(workspace!, "src/pipeline"), { recursive: true });
    await writeFile(join(workspace!, header), "#pragma once\n\ntypedef struct cbm_store cbm_store_t;\n");
    await writeFile(join(workspace!, other), "typedef struct cbm_store cbm_store_t;\n");
    await writeFile(join(workspace!, source), '#include "store.h"\nstruct cbm_store {\n    int handle;\n    int open;\n};\n');

    // The same typedef in two headers is one type, and the struct is what was asked for.
    const read = await readLocators(context, ["cbm_store_t"]);

    expect(read.ok).toBe(true);
    expect(read.text).not.toContain("# ambiguous");
    expect(read.text).toContain("typedef struct cbm_store cbm_store_t;");
    expect(read.text).toContain(`cbm_store Class ${source}:2-5`);
    expect(read.text).toContain("    int handle;");
    expect(read.text).toContain(`# also declared: ${other}:1`);
  });

  it("reads symbols from files created after indexing", async () => {
    const context = await createContext();
    const created = "src/main/java/com/shop/Refunds.java";
    await writeFile(
      join(context.root, created),
      ["package com.shop;", "", "class Refunds {", "  /** Half back. */", "  int half(int cents) {", "    return cents / 2;", "  }", "}"].join("\n"),
    );
    context.newFiles = { list: async () => [created], all: async () => [created] } as unknown as CodeContext["newFiles"];

    const type = await readLocators(context, ["Refunds"]);
    expect(type.ok).toBe(true);
    expect(type.text).toContain(`# Refunds ${created}:1-8`);
    expect(type.text).toContain("not indexed yet");

    const member = await readLocators(context, ["Refunds.half"]);
    expect(member.text).toContain(`# Refunds.half ${created}:4-7`);
    expect(member.text).not.toContain("class Refunds");

    expect((await readLocators(context, ["half"])).text).toContain(`${created}:4-7`);
    expect((await readLocators(context, ["Refunds.java:5-5"])).text).toContain("int half(int cents) {");
    expect((await readLocators(context, ["Nope.nothing"])).ok).toBe(false);
  });

  it("answers with the only similarly named symbol", async () => {
    const context = await createContext();

    const result = await readLocators(context, ["Ledgers"]);

    expect(result.ok).toBe(true);
    expect(result.text).toContain("# not found: Ledgers; showing the only similar name, Ledger");
    expect(result.text).toContain("# Ledger Interface");
    expect(similarNamePattern("PaymentController")).toBe("(?i)^Payment.{0,3}Controller.{0,3}$");
    expect(similarNamePattern("Ledgers")).toBe("(?i)^Ledgers?.{0,3}$");
    expect(new RegExp("^Payment.{0,3}Controller.{0,3}$", "i").test("PaymentsController")).toBe(true);
  });

  it("lists a directory and reads files the index does not hold", async () => {
    const context = await createContext();
    await mkdir(join(context.root, "scripts/ci"), { recursive: true });
    await writeFile(join(context.root, "scripts/build.sh"), "#!/bin/sh\nmake all");
    await writeFile(join(context.root, "scripts/ci/lint.sh"), "#!/bin/sh");
    const files = [BILLING, "scripts/build.sh", "scripts/ci/lint.sh"];
    context.newFiles = { list: async () => [], all: async () => files } as unknown as CodeContext["newFiles"];

    const directory = await readLocators(context, ["scripts"]);
    expect(directory.ok).toBe(true);
    expect(directory.text).toContain("# directory scripts/ files=2");
    expect(directory.text).toContain("scripts/ci/\t1 files");
    expect(directory.text).toContain("scripts/build.sh\t2 lines");
    expect((await readLocators(context, ["scripts/ci/"])).text).toContain("scripts/ci/lint.sh\t1 lines");

    const script = await readLocators(context, ["build.sh"]);
    expect(script.ok).toBe(true);
    expect(script.text).toContain("# scripts/build.sh:1-2");

    // A file name without an extension is still a file.
    await writeFile(join(context.root, "scripts/Makefile"), "all:\n\tcc main.c");
    files.push("scripts/Makefile");
    expect((await readLocators(context, ["Makefile"])).text).toContain("# scripts/Makefile:1-2");
    expect((await readLocators(context, ["missing/dir"])).ok).toBe(false);
  });

  it("shows every small file a bare file name stands for", async () => {
    const context = await createContext();
    await mkdir(join(context.root, "a"));
    await mkdir(join(context.root, "b"));
    await writeFile(join(context.root, "a/util.ts"), "export const a = 1;");
    await writeFile(join(context.root, "b/util.ts"), "export const b = 2;");
    const files = ["a/util.ts", "b/util.ts"];
    context.newFiles = { list: async () => [], all: async () => files } as unknown as CodeContext["newFiles"];

    const both = await readLocators(context, ["util.ts"]);
    expect(both.ok).toBe(true);
    expect(both.text).toContain("# util.ts names 2 files; all shown");
    expect(both.text).toContain("export const a = 1;");
    expect(both.text).toContain("# b/util.ts:1-1");

    await writeFile(join(context.root, "b/util.ts"), Array.from({ length: 600 }, () => "x").join("\n"));
    context.reader = new SourceReader(context.root);
    const listed = await readLocators(context, ["util.ts"]);
    expect(listed.ok).toBe(false);
    expect(listed.text).toContain("# ambiguous file: util.ts names 2 files; pick one by path\na/util.ts\nb/util.ts");
  });

  it("finds a Go method by its receiver type", async () => {
    const engine = "internal/sync/engine.go";
    const go = (label: string, qualified: string, startLine: number, endLine: number): GraphNode => ({
      qualifiedName: `${PROJECT}.internal.sync.${qualified}`,
      name: qualified.split(".").pop() ?? qualified,
      label,
      filePath: engine,
      startLine,
      endLine,
    });
    const context = await createContext({
      extraNodes: [
        go("Class", "Engine", 3, 9),
        go("Struct", "engine.Engine", 3, 9),
        go("Method", "emit", 11, 13),
        node(BILLING, "Method", ["Billing", "emit"], 18, 18),
      ],
    });
    const locate = (raw: string) => resolveSymbol(context.client, PROJECT, parseSymbolLocator(raw));

    // One struct indexed as Class and Struct is one symbol.
    expect(await locate("Engine")).toMatchObject({ kind: "resolved", nodes: [{ startLine: 3 }] });
    expect(await locate("sync.Engine.emit")).toMatchObject({
      kind: "resolved",
      nodes: [{ filePath: engine, startLine: 11 }],
    });
    // `Billing.emit` sits beside `Ledger` but belongs to another type.
    expect((await locate("Ledger.emit")).kind).toBe("not-found");
  });

  it("picks the Go method in the receiver's own file among same-named package symbols", async () => {
    const go = (label: string, file: string, qualified: string, startLine: number): GraphNode => ({
      qualifiedName: `${PROJECT}.internal.sync.${qualified}`,
      name: qualified.split(".").pop() ?? qualified,
      label,
      filePath: `internal/sync/${file}`,
      startLine,
      endLine: startLine + 4,
    });
    const context = await createContext({
      extraNodes: [
        go("Struct", "watcher.go", "Watcher", 10),
        go("Method", "watcher.go", "flush", 357),
        go("Function", "engine.go", "NewEngine.flush", 291),
      ],
    });

    expect(
      await resolveSymbol(context.client, PROJECT, parseSymbolLocator("Watcher.flush")),
    ).toMatchObject({ kind: "resolved", nodes: [{ filePath: "internal/sync/watcher.go", startLine: 357 }] });
  });

  it("gives small items their full size and splits the rest of the budget", () => {
    expect(fairBudgets([10, 1000, 100], 300)).toEqual([10, 190, 100]);
    expect(fairBudgets([50, 60], 300)).toEqual([50, 60]);
    expect(fairBudgets([400, 400], 300)).toEqual([150, 150]);
  });
});

describe("exploring code", () => {
  const backend = (overrides: Partial<GcalBackendClient>): GcalBackendClient =>
    ({
      search: async () => [],
      callers: async () => [],
      callees: async () => [],
      ...overrides,
    }) as unknown as GcalBackendClient;

  const textHit = (target: GraphNode, lines: number[]) => ({
    file: target.filePath,
    label: target.label,
    node: target.name,
    qualified_name: target.qualifiedName,
    start_line: target.startLine,
    end_line: target.endLine,
    match_lines: lines,
  });

  it("lists the symbols of code before headings and keys of the same name", async () => {
    const context = await createContext();
    const row = (qualifiedName: string, label: string, filePath: string) => ({
      qualifiedName,
      label,
      filePath,
      line: 1,
      signature: "",
    });
    const search = async () => [
      row("messages.en.session", "Variable", "frontend/messages/en.json"),
      row("messages.fr.session", "Variable", "frontend/messages/fr.json"),
      row("docs.usage.Session", "Section", "docs/usage.md"),
      row("store.SessionStore", "Class", "src/store/SessionStore.java"),
      row("store.session", "Function", "src/store/session.ts"),
    ];

    const result = await searchCode(context, backend({ search }), { queries: ["session"], limit: 3 });

    expect(result.text.split("\n").slice(0, 3).map((line) => line.split("\t").slice(0, 2).join(" "))).toEqual([
      "store.session Function",
      "store.SessionStore Class",
      "en.session Variable",
    ]);

    // A full first page of keys: the code behind it is asked for once more.
    const asked: number[] = [];
    const crowded = async (_query: string, options: { limit: number }) => {
      asked.push(options.limit);
      const keys = Array.from({ length: options.limit }, (_, index) =>
        row(`messages.l${index}.session`, "Variable", `frontend/messages/l${index}.json`),
      );
      return options.limit > 20 ? [...keys.slice(0, 15), row("store.session", "Function", "src/store/session.ts")] : keys;
    };
    const behind = await searchCode(context, backend({ search: crowded }), { queries: ["session"], limit: 3 });
    expect(asked).toEqual([15, 200]);
    expect(behind.text.split("\n")[0]).toContain("store.session\tFunction");
  });

  it("collapses overloads into one search row and falls back to text", async () => {
    const context = await createContext({ textResults: [textHit(NODES[5], [15])] });
    const search = async (query: string) =>
      query === "charge"
        ? [NODES[3], NODES[4]].map((item) => ({
            qualifiedName: item.qualifiedName,
            label: item.label,
            filePath: item.filePath,
            line: item.startLine,
            signature: "",
          }))
        : [];

    const result = await searchCode(context, backend({ search }), {
      queries: ["charge", "-cents"],
      limit: 5,
    });

    expect(result.text).toContain(
      `Billing.charge\tMethod\t${BILLING}:6\tpublic void charge(int cents)\t(2 overloads)`,
    );
    expect(result.text).toContain('# no symbol named "-cents"; text matches:');
    // The line keeps its indentation: an edit is written from this text.
    expect(result.text).toContain(`${BILLING}\n  15\tBilling.refund\t    record(-cents);`);

    // A large --limit stays within the page size the backend accepts.
    const asked: number[] = [];
    await searchCode(
      context,
      backend({
        search: async (_query: string, options: { limit: number }) => {
          asked.push(options.limit);
          return [];
        },
      }),
      { queries: ["charge"], limit: 80 },
    );
    expect(asked).toEqual([200]);

    // A search word is often where something lives: files named like it are listed.
    context.newFiles = {
      list: async () => [],
      all: async () => [BILLING, BILLING_TEST, "docs/billing-guide.md", "assets/billing.png", LEDGER],
    } as unknown as CodeContext["newFiles"];
    const named = await searchCode(context, backend({ search }), { queries: ["billing", "a|b"], limit: 5 });
    expect(named.text).toContain(
      `# files named like "billing": docs/billing-guide.md, ${BILLING}, ${BILLING_TEST}`,
    );
    expect(named.text).not.toContain('files named like "a|b"');
  });

  it("adds text references the call graph missed and skips declarations", async () => {
    const record = NODES.find((item) => item.name === "record")!;
    const context = await createContext({
      textResults: [textHit(NODES[3], [7]), textHit({ ...record, filePath: BILLING, startLine: 18, endLine: 18 }, [18])],
    });

    const result = await references(context, backend({}), {
      target: "Ledger.record",
      depth: 1,
      limit: 20,
      includeText: true,
    });

    expect(result.text).toBe(
      [
        "# callers of Ledger.record: 1 (graph 0, text-only 1)",
        BILLING,
        "  7\tBilling.charge\ttext\t    record(cents);",
      ].join("\n"),
    );

    // The backend read fewer places than matched: the answer says so.
    const cut = await createContext({ textResults: [textHit(NODES[3], [7])], textTotal: 90 });
    const partial = await references(cut, backend({}), {
      target: "Ledger.record",
      depth: 1,
      limit: 20,
      includeText: true,
    });
    expect(partial.text.split("\n").at(-1)).toBe(
      "# text references: the first 1 of 90 places were read; gcal grep '\\brecord\\s*\\(' lists every line",
    );
  });

  it("gives every use its own row when no symbol names the place, and leaves the declaration out", async () => {
    // A file changed after indexing: the search reports lines without a symbol.
    const context = await createContext({
      textResults: [{ file: BILLING, label: "", node: "", qualified_name: "", start_line: 0, end_line: 0, match_lines: [6, 7, 15] }],
    });
    const charge = NODES.find((item) => item.qualifiedName.endsWith("Billing.charge"))!;
    const fileLines = BILLING_SOURCE.split("\n");
    const result = await references(context, backend({}), {
      target: "Ledger.record",
      depth: 1,
      limit: 20,
      includeText: true,
    });
    const rows = result.text.split("\n");
    expect(rows[0]).toBe("# callers of Ledger.record: 3 (graph 0, text-only 3)");
    // One row per line, each with its own text.
    expect(rows.slice(2)).toEqual([6, 7, 15].map((line) => `  ${line}\t\ttext\t${fileLines[line - 1]}`));
    expect(charge).toBeDefined();

    // The line that declares the symbol is not one of its uses.
    const own = await createContext({
      extraNodes: [node(BILLING, "Method", ["Billing", "record"], 18, 18)],
      textResults: [{ file: BILLING, label: "", node: "", qualified_name: "", start_line: 0, end_line: 0, match_lines: [7, 18] }],
    });
    const declared = await references(own, backend({}), { target: "Billing.record", depth: 1, limit: 20, includeText: true });
    expect(declared.text.split("\n").slice(2)).toEqual([`  7\t\ttext\t${fileLines[6]}`]);
  });

  it("finds a call of the graph again when its line moved since indexing", async () => {
    const context = await createContext();
    const refund = NODES.find((item) => item.qualifiedName.endsWith("Billing.refund"))!;
    const record = NODES.find((item) => item.name === "record")!;
    // The index says line 3; the call is on line 15 of the file as it is.
    const stale = backend({
      callers: async () => [
        {
          sourceQualifiedName: refund.qualifiedName,
          targetQualifiedName: record.qualifiedName,
          relatedQualifiedName: refund.qualifiedName,
          hop: 1,
          filePath: BILLING,
          line: 3,
        },
      ],
    });

    const result = await references(context, stale, { target: "Ledger.record", depth: 1, limit: 20, includeText: false });

    expect(result.text).toBe(
      ["# callers of Ledger.record: 1 (graph 1, text-only 0)", BILLING, "  15\tBilling.refund\tcall\t    record(-cents);"].join(
        "\n",
      ),
    );
  });

  it("lists subtypes from the graph", async () => {
    const ledger = NODES.find((item) => item.label === "Interface")!;
    const context = await createContext({ subtypes: { [ledger.qualifiedName]: [NODES[1]] } });

    const result = await implementations(context, "Ledger", 1);

    expect(result.text).toBe(
      `# subtypes of Ledger: 1\nBilling\tClass\t${BILLING}:3-20\timplements Ledger`,
    );
  });

  it("asks a Go interface's methods of their receivers, and takes the type when a field has its name", async () => {
    const engine = "internal/sync/engine.go";
    const emitter = node(engine, "Interface", ["Emitter"], 3, 6);
    const field = node(engine, "Field", ["EngineConfig", "Emitter"], 9, 9);
    const emit = node("internal/server/broadcaster.go", "Method", ["Emit"], 67, 69);
    const context = await createContext({ extraNodes: [emitter, field], textResults: [textHit(emit, [67])] });
    await mkdir(join(workspace!, "internal/sync"), { recursive: true });
    await writeFile(
      join(workspace!, engine),
      "package sync\n\ntype Emitter interface {\n\tEmit(scope string)\n\tClose() error\n}\n\ntype EngineConfig struct {\n\tEmitter Emitter\n}\n",
    );

    const result = await implementations(context, "Emitter", 1);

    expect(result.ok).toBe(true);
    expect(result.text).not.toContain("# ambiguous");
    expect(result.text.split("\n")[0]).toBe(
      "# subtypes of go.Emitter: none in graph; a Go type satisfies it by having Emit, Close; methods of that name by receiver:",
    );
    expect(result.text).toContain("internal/server/broadcaster.go");
  });

  it("reports when text search finds nothing", async () => {
    const context = await createContext();
    expect((await grepCode(context, { pattern: "zzz", regex: false, limit: 10 })).text).toBe(
      "# no text matches",
    );
  });

  it("names the matching files that a cut search does not show", async () => {
    const context = await createContext({
      textResults: [textHit(NODES[3], [7]), textHit(NODES[4], [11]), textHit(NODES[5], [15])],
    });
    const other = "src/main/java/com/shop/Refunds.java";
    await mkdir(join(context.root, "docs"));
    await writeFile(join(context.root, other), "class Refunds {\n  void undo() { record(-1); }\n}");
    await writeFile(join(context.root, "docs/guide.md"), "Call `record(cents)` to book.");
    await writeFile(join(context.root, "src/Audit.java"), "class Audit { void log() { record(0); } }");
    context.newFiles = {
      list: async () => [],
      all: async () => [BILLING, other, "src/Audit.java", "docs/guide.md"],
    } as unknown as CodeContext["newFiles"];
    const request = { pattern: "record(", regex: true, paths: ["src"], limit: 1 };
    const footer = [
      "# ... 4 more matches not shown; give paths or raise --limit",
      `# matching files not shown (path matching-lines): ${BILLING} 4, src/Audit.java 1`,
    ];

    const withContext = await grepCode(context, { ...request, context: 0 });
    expect(withContext.text).toBe([other, "2:  void undo() { record(-1); }", ...footer].join("\n"));

    const rows = await grepCode(context, request);
    expect(rows.text.split("\n").slice(-2)).toEqual(footer);

    // Files of one directory are listed under it.
    const scoped = await grepCode(context, { ...request, paths: ["src/main"], limit: 2 });
    expect(scoped.text).not.toContain("# matching files not shown");
    context.newFiles = {
      list: async () => [],
      all: async () => [other, "src/Audit.java", "src/Extra.java"],
    } as unknown as CodeContext["newFiles"];
    await writeFile(join(context.root, "src/Extra.java"), "record(1);\nrecord(2);");
    const grouped = await grepCode(context, { ...request, paths: ["src"], limit: 1 });
    expect(grouped.text.split("\n").at(-1)).toBe(
      "# matching files not shown (path matching-lines): src/{Audit.java 1, Extra.java 2}",
    );

    // Nothing is cut: no scan and no footer.
    const whole = await grepCode(context, { ...request, limit: 10 });
    expect(whole.text).not.toContain("# matching files not shown");
  });

  it("shows every match the backend returned unless a limit was asked for", async () => {
    const hits = Array.from({ length: 35 }, (_, index) =>
      textHit(node(BILLING, "Method", ["Billing", `step${index}`], index + 1, index + 1), [index + 1]),
    );
    const whole = await grepCode(await createContext({ textResults: hits }), { pattern: "record", regex: false });
    expect(whole.text.split("\n")).toHaveLength(36);
    expect(whole.text).not.toContain("more matches not shown");

    const asked = await grepCode(await createContext({ textResults: hits }), { pattern: "record", regex: false, limit: 30 });
    expect(asked.text).toContain("# ... 5 more matches not shown; give paths or raise --limit");

    // The backend matched more than it returned: the default limit applies.
    const partial = await grepCode(await createContext({ textResults: hits, textTotal: 200 }), {
      pattern: "record",
      regex: false,
    });
    expect(partial.text).toContain("# ... 170 more matches not shown; give paths or raise --limit");
  });

  it("reads doubled backslashes as single ones when the pattern matches nothing", async () => {
    const context = await createContext({ textResults: [] });
    const created = "src/main/java/com/shop/Refunds.java";
    await writeFile(join(context.root, created), "class Refunds {\n  void undo() { record(-1); }\n}");
    context.newFiles = {
      list: async () => [created],
      all: async () => [BILLING, created],
    } as unknown as CodeContext["newFiles"];

    const result = await grepCode(context, { pattern: "\\\\brecord\\\\(-1", regex: true, limit: 10, context: 0 });

    expect(result.text).toBe(
      [
        "# no match for \\\\brecord\\\\(-1; read as \\brecord\\(-1 (doubled backslashes)",
        created,
        "2:  void undo() { record(-1); }",
      ].join("\n"),
    );
    expect((await grepCode(context, { pattern: "\\\\bnothing\\\\b", regex: true, limit: 10 })).text).toBe(
      "# no text matches",
    );
  });

  it("resolves a call target given as a line or as a file-qualified name", async () => {
    const constant = (filePath: string, startLine: number): GraphNode => ({
      qualifiedName: [PROJECT, ...filePath.replace(/\.ts$/, "").split("/"), "native"].join("."),
      name: "native",
      label: "Variable",
      filePath,
      startLine,
      endLine: startLine + 2,
    });
    const bridge = constant("src/lib/native.ts", 4);
    const context = await createContext({
      extraNodes: [bridge, constant("src/app/useSwitcher.ts", 9), constant("src/app/shell.ts", 3)],
    });
    const callersOf = async (target: string) =>
      (await references(context, backend({}), { target, depth: 1, limit: 20, includeText: false })).text;

    const ambiguous = await callersOf("native");
    expect(ambiguous).toContain(
      "# ambiguous: native matches 3 symbols; name its owner (Type.member, file.ext.member) or give its path:line",
    );
    expect(ambiguous).toContain("src/lib/native.ts:4");

    expect(await callersOf("native.ts.native")).toBe("# callers of native: 0 (graph 0, text-only 0)");
    expect(await callersOf(`${BILLING}:7`)).toBe("# callers of Billing.charge: 0 (graph 0, text-only 0)");
    expect(await callersOf(`${BILLING}:2`)).toBe(`# not found: no indexed symbol at ${BILLING}:2`);
  });

  it("lists the text uses of a symbol the call graph does not hold", async () => {
    const constant: GraphNode = {
      qualifiedName: `${PROJECT}.src.lib.native.native`,
      name: "native",
      label: "Variable",
      filePath: "src/lib/native.ts",
      startLine: 4,
      endLine: 6,
    };
    const context = await createContext({ extraNodes: [constant], textResults: [textHit(NODES[3], [7])] });
    const failing = (message: string) =>
      backend({
        callers: async () => {
          throw new Error(message);
        },
      });
    const request = { target: "native", depth: 1, limit: 20, includeText: true };

    const result = await references(context, failing("MCP tool error: symbol was not found: native"), request);
    expect(result.text).toBe(
      ["# callers of native: 1 (graph 0, text-only 1)", BILLING, "  7\tBilling.charge\ttext\t    record(cents);"].join(
        "\n",
      ),
    );
    await expect(references(context, failing("backend exited"), request)).rejects.toThrow("backend exited");

    expect(new RegExp(sameLanguageFilter("src/lib/native.ts")!).test("src/app/App.tsx")).toBe(true);
    expect(new RegExp(sameLanguageFilter("src/lib/native.ts")!).test("flake.nix")).toBe(false);
    expect(sameLanguageFilter("src/lib/store.zig")).toBe("\\.(zig)$");
    expect(sameLanguageFilter("Makefile")).toBeUndefined();
  });

  it("lists the files that import a module, under every spelling of its path", async () => {
    const context = await createContext();
    const files: Record<string, string> = {
      "src/lib/native.ts": "export const native = {};\nexport type Entry = string;\n",
      "src/app/App.tsx": [
        'import { cn } from "@/lib/utils";',
        "import {",
        "  native,",
        "  type Entry as Row,",
        '} from "@/lib/native";',
        "const rows: Row[] = [];",
        "native.open(rows);",
      ].join("\n"),
      "src/lib/transport.ts": 'import { native } from "./native.js";\nexport const send = () => native.send();\n',
      "src/lib/transport.test.ts": 'vi.mock("./native", () => ({\n  native: { send: vi.fn() },\n}));\n',
      // Another module with the same base name, and the bare word in prose.
      "src/os/native.ts": "export const os = 1;\n",
      "src/os/use.ts": 'import { os } from "./native";\nconst label = "native";\n',
      "src/app/lazy.ts": 'const loaded = await import("../lib/native");\n',
    };
    for (const [path, text] of Object.entries(files)) {
      await mkdir(join(context.root, dirname(path)), { recursive: true });
      await writeFile(join(context.root, path), text);
    }
    context.newFiles = { list: async () => [], all: async () => Object.keys(files) } as unknown as CodeContext["newFiles"];
    const callersOf = async (target: string) =>
      (await references(context, backend({}), { target, depth: 1, limit: 20, includeText: true })).text;

    const listed = [
      '"@/lib/native" 1, "../lib/native" 1, "./native.js" 1, "./native" 1',
      "# imported names (files): native 2, Entry 1",
      "src/app/App.tsx",
      '  5\t} from "@/lib/native";',
      "  uses\tnative 7; Row 6",
      "src/app/lazy.ts",
      '  1\tconst loaded = await import("../lib/native");',
      "  uses\tloaded -",
      "src/lib/transport.ts",
      '  1\timport { native } from "./native.js";',
      "  uses\tnative 2",
      "src/lib/transport.test.ts",
      '  1\tvi.mock("./native", () => ({',
      "  uses\tnative 2",
    ].join("\n");
    expect(await callersOf("src/lib/native.ts")).toBe(
      `# importers of src/lib/native.ts: 4 files (1 tests); written as ${listed}`,
    );

    // After the move the old path is still what the files name.
    await rm(join(context.root, "src/lib/native.ts"));
    expect(await callersOf("src/lib/native.ts")).toBe(
      `# importers of src/lib/native.ts (no such file now): 4 files (1 tests); written as ${listed}`,
    );
    expect(await callersOf("src/lib/nativeBridge.ts")).toBe(
      "# importers of src/lib/nativeBridge.ts (no such file now): 0 (scanned 7 files)",
    );
  });

  it("lists Python importers, and answers a Java file with the callers of its type", async () => {
    const context = await createContext();
    const files: Record<string, string> = {
      "shop/pairs.py": "def pair():\n    return 1\n",
      "shop/cart.py": "from .pairs import pair as two\nvalue = two()\n",
      "shop/api/views.py": "from shop.pairs import (\n    pair,\n)\nfrom .. import pairs\nprint(pair(), pairs)\n",
      "tools/run.py": "import shop.pairs as p\nimport pairs\np.pair()\n",
    };
    for (const [path, text] of Object.entries(files)) {
      await mkdir(join(context.root, dirname(path)), { recursive: true });
      await writeFile(join(context.root, path), text);
    }
    context.newFiles = { list: async () => [], all: async () => Object.keys(files) } as unknown as CodeContext["newFiles"];
    const callersOf = async (target: string) =>
      (await references(context, backend({}), { target, depth: 1, limit: 20, includeText: false })).text;

    expect(await callersOf("shop/pairs.py")).toBe(
      [
        '# importers of shop/pairs.py: 3 files; written as "shop.pairs" 2, ".. import pairs" 1, ".pairs" 1, "pairs" 1',
        "# imported names (files): pair 2",
        "shop/api/views.py",
        "  1\tfrom shop.pairs import (",
        "  4\tfrom .. import pairs",
        "  uses\tpair 5; pairs 5",
        "shop/cart.py",
        "  1\tfrom .pairs import pair as two",
        "  uses\ttwo 2",
        "tools/run.py",
        "  1\timport shop.pairs as p",
        // A bare module name is trusted because the target sits near the root.
        "  2\timport pairs",
        "  uses\tp 3; pairs -",
      ].join("\n"),
    );

    expect(await callersOf(BILLING)).toBe("# callers of Billing: 0 (graph 0, text-only 0)");
  });

  it("adds to an outlined type the members that call a symbol read with it", async () => {
    const BIG = "src/test/java/com/shop/BigTests.java";
    const big = (rest: string[], label: string, start: number, end: number) => node(BIG, label, rest, start, end);
    const context = await createContext({
      extraNodes: [
        big(["BigTests"], "Class", 3, 600),
        big(["BigTests", "refundsTwice"], "Method", 10, 14),
        // A lambda inside the test above: its use belongs to that test.
        big(["BigTests", "refundsTwice", "lambda"], "Function", 12, 12),
        big(["BigTests", "unrelated"], "Method", 20, 22),
        big(["BigTests", "refundsLater"], "Method", 30, 32),
      ],
    });
    const lines = Array.from({ length: 600 }, () => "");
    lines[2] = "class BigTests {";
    lines[9] = "  void refundsTwice() {";
    lines[10] = "    billing.refund(1);";
    lines[11] = "    run(() -> billing.refund(2));";
    lines[13] = "  }";
    lines[19] = "  void unrelated() {";
    lines[20] = "    billing.charge(1);";
    lines[21] = "  }";
    lines[29] = "  void refundsLater() {";
    lines[30] = "    billing.refund(3);";
    lines[31] = "  }";
    lines[599] = "}";
    await mkdir(join(context.root, dirname(BIG)), { recursive: true });
    await writeFile(join(context.root, BIG), lines.join("\n"));

    const read = await readLocators(context, ["Billing.refund", "BigTests"]);

    expect(read.text).toContain("# outline BigTests Class");
    expect(read.text).toContain(
      [
        "# in BigTests, 2 members call refund",
        "",
        `# BigTests.refundsTwice Method ${BIG}:10-14`,
        "  void refundsTwice() {",
        "    billing.refund(1);",
        "    run(() -> billing.refund(2));",
        "",
        "  }",
        "",
        `# BigTests.refundsLater Method ${BIG}:30-32`,
        "  void refundsLater() {",
        "    billing.refund(3);",
        "  }",
      ].join("\n"),
    );
    // The other member stays a row of the outline: its body is not printed.
    expect(read.text).not.toContain("billing.charge(1);");

    // Asked for alone, the type is only outlined; a small type is printed whole anyway.
    expect((await readLocators(context, ["BigTests"])).text).not.toContain("members call");
    expect((await readLocators(context, ["Billing.refund", "BillingTests"])).text).not.toContain(
      "members call",
    );
    // The file form of the same request behaves the same way.
    expect((await readLocators(context, ["Billing.refund", BIG])).text).toContain(
      `# in ${BIG}, 2 members call refund`,
    );
  });

  it("prints the tests that use a symbol, whole when they are short", async () => {
    const testMethod = NODES.find((item) => item.qualifiedName.endsWith("BillingTests.refund"))!;
    const context = await createContext({
      // One use in a test, one in production code.
      textResults: [textHit(NODES[3], [7]), textHit(testMethod, [6])],
    });
    await mkdir(join(context.root, dirname(BILLING_TEST)), { recursive: true });
    await writeFile(
      join(context.root, BILLING_TEST),
      [
        "package com.shop;",
        "",
        "class BillingTests {",
        "  @Test",
        "  void refund() {",
        "    new Billing().refund(5);",
        "    assertEquals(-5, total());",
        "  }",
        "}",
      ].join("\n"),
    );

    const tests = await testsUsing(context, "Billing.refund");

    // The only test file that uses it is small: it is printed whole.
    expect(tests).toBe(
      [
        `# tests using Billing.refund: 1 in ${BILLING_TEST}, printed whole (lines 6)`,
        "",
        `# ${BILLING_TEST}:1-9`,
        "package com.shop;",
        "",
        "class BillingTests {",
        "  @Test",
        "  void refund() {",
        "    new Billing().refund(5);",
        "    assertEquals(-5, total());",
        "  }",
        "}",
      ].join("\n"),
    );

    // The backend refuses the lookup of the enclosing test on a very large index:
    // the symbol the search reported is used as it is.
    const lookup = context.client.tool.bind(context.client);
    context.client = {
      tool: async (name, args) => {
        if (name === "query_graph" && String(args.query).includes("start_line <=")) {
          throw new Error("MCP tool error: unsupported Cypher query: query exceeds intermediate binding safety cap");
        }
        return lookup(name, args);
      },
    };
    // A test file is printed once per answer.
    expect(await testsUsing(context, "Billing.refund")).toBe(
      `# tests using Billing.refund: 1 in ${BILLING_TEST}, printed above (lines 6)`,
    );

    const unused = await createContext({ textResults: [textHit(NODES[3], [7])] });
    expect(await testsUsing(unused, "Billing.refund")).toBe("# tests using Billing.refund: none found");
    // A file has no tests of its own to list, and an unknown name is reported by `get` itself.
    // A Java file is answered through the type it declares.
    expect(await testsUsing(unused, BILLING)).toBe("# tests using Billing: none found");
    expect(await testsUsing(unused, "Nope.missing")).toBeUndefined();

    // With no test of the symbol, the closest test files say where one would go
    // and the shortest shows what the project's tests look like.
    const far = "src/test/java/com/other/FarTests.java";
    const ledgerTest = "src/test/java/com/shop/LedgerTests.java";
    const ledgerSource = ["package com.shop;", "", "class LedgerTests {", "  @Test", "  void records() {}", "}"];
    for (const [path, text] of [
      [far, "class FarTests {}\n"],
      [ledgerTest, `${ledgerSource.join("\n")}\n`],
    ]) {
      await mkdir(join(unused.root, dirname(path)), { recursive: true });
      await writeFile(join(unused.root, path), text);
    }
    unused.newFiles = {
      list: async () => [],
      all: async () => [BILLING, far, ledgerTest, "docs/test/notes.md"],
    } as unknown as CodeContext["newFiles"];
    expect(await testsUsing(unused, "Billing.refund")).toBe(
      [
        "# tests using Billing.refund: none found",
        `# nearest test files: ${ledgerTest} (6 lines), ${far} (1 line)`,
        "",
        `# example of this project's tests: ${far}:1-1`,
        "class FarTests {}",
      ].join("\n"),
    );
    // The example is printed once per answer.
    expect(await testsUsing(unused, "Billing.charge")).toBe(
      ["# tests using Billing.charge: none found", `# nearest test files: ${ledgerTest} (6 lines), ${far} (1 line)`].join("\n"),
    );
  });

  it("keeps the tests of a method to files that name its type, and is brief for a type or a large batch", async () => {
    const testMethod = NODES.find((item) => item.qualifiedName.endsWith("BillingTests.refund"))!;
    const OTHER = "src/test/java/com/shop/WalletTests.java";
    const elsewhere = node(OTHER, "Method", ["WalletTests", "refundsElsewhere"], 2, 4);
    const prepare = async () => {
      const context = await createContext({ textResults: [textHit(elsewhere, [3]), textHit(testMethod, [6])] });
      for (const [path, text] of [
        [BILLING_TEST, "package com.shop;\n\nclass BillingTests {\n  @Test\n  void refund() {\n    new Billing().refund(5);\n  }\n}\n"],
        // Another `refund(`, on something that is not a Billing.
        [OTHER, "class WalletTests {\n  void refundsElsewhere() {\n    wallet.refund(1);\n  }\n}\n"],
      ]) {
        await mkdir(join(context.root, dirname(path)), { recursive: true });
        await writeFile(join(context.root, path), text);
      }
      return context;
    };

    const method = await testsUsing(await prepare(), "Billing.refund");
    expect(method!.split("\n")[0]).toBe(`# tests using Billing.refund: 1 in ${BILLING_TEST}, printed whole (lines 6)`);
    expect(method).not.toContain("WalletTests");

    // One name of a large batch: where its tests are, and one of them whole as
    // an example of how this project writes a test.
    const batch = await prepare();
    expect(await testsUsing(batch, "Billing.refund", { brief: true })).toBe(
      [
        `# tests using Billing.refund: 1 in 1 file`,
        `  ${BILLING_TEST}:6`,
        "",
        "# one of them, as an example of how they are written:",
        "",
        // The fixture's index gives the method one line more than it has.
        `# BillingTests.refund Method ${BILLING_TEST}:4-8`,
        "  @Test",
        "  void refund() {",
        "    new Billing().refund(5);",
        "  }",
        "}",
        "",
        `# head of ${BILLING_TEST}:1-3`,
        "package com.shop;",
        "",
        "class BillingTests {",
      ].join("\n"),
    );
    // The example comes once per answer: the next name gets locations only.
    expect(await testsUsing(batch, "Billing.refund", { brief: true })).toBe(
      [`# tests using Billing.refund: 1 in 1 file`, `  ${BILLING_TEST}:6`].join("\n"),
    );

    // The tests of a type are its test files.
    const ofType = await testsUsing(await prepare(), "Billing");
    expect(ofType!.split("\n").slice(0, 3)).toEqual([
      `# tests using Billing: 2 in 2 files`,
      `  ${BILLING_TEST}:6`,
      `  ${OTHER}:3`,
    ]);
    expect(ofType).toContain("# one of them, as an example of how they are written:");
  });

  it("answers --tests on a file with the test files that import it", async () => {
    const context = await createContext();
    const files: Record<string, string> = {
      "shop/pairs.py": "def pair():\n    return 1\n",
      "shop/cart.py": "from .pairs import pair\n",
      "tests/test_pairs.py": "from shop.pairs import pair\n\n\ndef test_pair():\n    assert pair() == 1\n",
      "tests/test_other.py": "def test_other():\n    assert True\n",
    };
    for (const [path, text] of Object.entries(files)) {
      await mkdir(join(context.root, dirname(path)), { recursive: true });
      await writeFile(join(context.root, path), text);
    }
    context.newFiles = { list: async () => [], all: async () => Object.keys(files) } as unknown as CodeContext["newFiles"];

    expect(await testsUsing(context, "shop/pairs.py")).toBe(
      [
        '# tests importing shop/pairs.py: 1 files; written as "shop.pairs" 1',
        "# imported names (files): pair 1",
        "tests/test_pairs.py",
        "  1\tfrom shop.pairs import pair",
        "  uses\tpair 5",
        "",
        // The one test file that imports it, whole.
        "# tests/test_pairs.py:1-5",
        "from shop.pairs import pair",
        "",
        "",
        "def test_pair():",
        "    assert pair() == 1",
      ].join("\n"),
    );
    // No test imports it: where its tests would go.
    expect(await testsUsing(context, "shop/cart.py")).toBe(
      [
        "# tests importing shop/cart.py: none found",
        "# nearest test files: tests/test_other.py (2 lines), tests/test_pairs.py (5 lines)",
        "",
        "# example of this project's tests: tests/test_other.py:1-2",
        "def test_other():",
        "    assert True",
      ].join("\n"),
    );
    // A test file has no tests of its own.
    expect(await testsUsing(context, "tests/test_pairs.py")).toBeUndefined();
    // The bare file name is enough.
    expect(await testsUsing(context, "pairs.py")).toContain("# tests importing shop/pairs.py: 1 files");
  });

  it("prints matching lines with context, merging overlapping windows", async () => {
    const context = await createContext({
      textResults: [textHit(NODES[3], [7]), textHit(NODES[4], [11]), textHit(NODES[5], [15])],
    });

    const result = await grepCode(context, { pattern: "record(", regex: false, limit: 10, context: 1 });

    expect(result.text).toBe(
      [
        BILLING,
        "6-  public void charge(int cents) {",
        "7:    record(cents);",
        "8-  }",
        "--",
        "10-  public void charge(int cents, String note) {",
        "11:    record(cents);",
        "12-  }",
        "--",
        "14-  public void refund(int cents) {",
        "15:    record(-cents);",
        "16-  }",
      ].join("\n"),
    );

    const merged = await grepCode(context, { pattern: "record(", regex: false, limit: 10, context: 2 });
    expect(merged.text).not.toContain("--");
    expect(merged.text.split("\n")).toHaveLength(1 + 13);
  });

  it("reads a pattern as a regex first and as literal text when that fails", async () => {
    const modes: boolean[] = [];
    const options = {
      textResults: [textHit(NODES[5], [15])],
      textSearchModes: modes,
      regexFindsNothing: false,
    };
    const context = await createContext(options);
    const modesFor = async (pattern: string, regex: boolean) => {
      modes.length = 0;
      const result = await grepCode(context, { pattern, regex, limit: 10 });
      expect(result.text).toContain(BILLING);
      return [...modes];
    };

    // Not a valid regex, and nothing regex-like: one literal search each.
    expect(await modesFor("record(", true)).toEqual([false]);
    expect(await modesFor("record", true)).toEqual([false]);
    expect(await modesFor("rec.rd", false)).toEqual([false]);
    expect(await modesFor("rec[o]rd", true)).toEqual([true]);

    options.regexFindsNothing = true;
    expect(await modesFor("rec[o]rd", true)).toEqual([true, false]);
  });

  it("searches files the index does not hold", async () => {
    const context = await createContext({ textResults: [] });
    const created = "src/main/java/com/shop/Refunds.java";
    await mkdir(join(context.root, "docs"));
    await writeFile(join(context.root, "docs/guide.md"), "intro\nCall `record(cents)` to book.");
    await writeFile(join(context.root, created), "class Refunds {\n  void undo() { record(-1); }\n}");
    context.newFiles = {
      list: async () => [created],
      all: async () => [BILLING, created, "docs/guide.md"],
    } as unknown as CodeContext["newFiles"];

    // A file created after indexing is always searched.
    const fresh = await grepCode(context, { pattern: "record\\(-1", regex: true, limit: 10 });
    expect(fresh.text).toContain(created);
    expect(fresh.text).not.toContain("docs/guide.md");

    // Paths also reach files outside the index.
    const scoped = await grepCode(context, {
      pattern: "record(",
      regex: true,
      paths: ["docs"],
      context: 0,
      limit: 10,
    });
    expect(scoped.text).toBe("docs/guide.md\n2:Call `record(cents)` to book.");

    // `.` is the whole repository, not a path that matches nothing.
    const everywhere = await grepCode(context, { pattern: "record(", regex: false, paths: ["."], limit: 10 });
    expect(everywhere.text).toContain(created);
  });

  it("turns rg-style paths into one backend path filter", () => {
    const filter = new RegExp(pathsPattern("/repo", ["Billing.java", "src/test/", "Ledger"]));

    expect(filter.test(BILLING)).toBe(true);
    expect(filter.test("src/main/java/com/shop/NotBilling.java")).toBe(false);
    expect(filter.test(BILLING_TEST)).toBe(true);
    expect(filter.test(LEDGER)).toBe(true);
    expect(filter.test("src/main/java/com/shop/LedgerImpl.java")).toBe(false);
  });

  it("splits a path list that arrived as one argument, and reads rg-style globs", async () => {
    const context = await createContext();
    await mkdir(join(context.root, "docs"));
    await writeFile(join(context.root, "docs/my notes.md"), "text");

    // A PowerShell array passed through a script is one argument joined by spaces.
    expect(separatePaths(context.root, [`${BILLING} docs`, "src"])).toEqual([BILLING, "docs", "src"]);
    // So is a list of absolute paths, as `Get-ChildItem | ForEach-Object FullName` gives.
    const absolute = [join(context.root, BILLING), join(context.root, "docs")];
    expect(separatePaths(context.root, [absolute.join(" ")])).toEqual(absolute);
    // A real path with a space, and words that are not all paths, stay as given.
    expect(separatePaths(context.root, ["docs/my notes.md", `${BILLING} nowhere`])).toEqual([
      "docs/my notes.md",
      `${BILLING} nowhere`,
    ]);

    const matches = (glob: string, path: string) => new RegExp(globPattern(glob)).test(path);
    expect(matches("*.py", "src/lod/pairs.py")).toBe(true);
    expect(matches("*.py", "src/lod/pairs.pyc")).toBe(false);
    expect(matches("src/**/*.ts", "src/app/hooks/use.ts")).toBe(true);
    expect(matches("src/**/*.ts", "lib/app/use.ts")).toBe(false);
    expect(matches("*.{ts,tsx}", "src/App.tsx")).toBe(true);
    // `-g '*.ts' -g '*.tsx'`: a file that matches either glob.
    const either = new RegExp(globsPattern(["*.ts", "*.tsx"])!);
    expect(["src/a.ts", "src/App.tsx", "src/a.py"].map((path) => either.test(path))).toEqual([true, true, false]);
    expect(globsPattern([])).toBeUndefined();
    expect(matches("tests/*.py", "tests/codec/test_a.py")).toBe(false);
  });

  it("keeps the rest of a batch when the backend cannot answer one item", async () => {
    const context = await createContext();
    const tool = context.client.tool.bind(context.client);
    context.client = {
      tool: async (name, args) => {
        if (name === "query_graph" && String(args.query).includes("Ledger.java")) {
          throw new Error("MCP tool error: unsupported Cypher query: query exceeds intermediate binding safety cap");
        }
        return tool(name, args);
      },
    };

    const batch = await readLocators(context, [`${BILLING}:7`, `${LEDGER}:4`]);

    expect(batch.ok).toBe(true);
    expect(batch.text).toContain("record(cents);");
    expect(batch.text).toContain(
      `# failed: ${LEDGER}:4: MCP tool error: unsupported Cypher query: query exceeds intermediate binding safety cap; read it by range: gcal get ${LEDGER}:1-400`,
    );
    // Alone, the failure is the command's own.
    await expect(readLocators(context, [`${LEDGER}:4`])).rejects.toThrow("safety cap");
  });

  it("summarizes the layout within a row budget, production first", () => {
    const paths = [
      ...Array.from({ length: 30 }, (_, index) => `app/src/main/core/a${index}.ts`),
      ...Array.from({ length: 20 }, (_, index) => `app/src/main/web/b${index}.ts`),
      ...Array.from({ length: 10 }, (_, index) => `app/src/test/core/c${index}.ts`),
      "README.md",
    ];

    expect(summarizeLayout(paths, 3)).toBe(
      [
        "# layout: 61 indexed files (ts 60, md 1)",
        "1\t./*",
        "30\tapp/src/main/core/",
        "20\tapp/src/main/web/",
        "10\tapp/src/test/core/",
      ].join("\n"),
    );
  });

  it("opens the directories of source before a larger directory of documents", () => {
    const paths = [
      ...Array.from({ length: 40 }, (_, index) => `docs/plans/p${index % 4}/note${index}.md`),
      ...Array.from({ length: 12 }, (_, index) => `ui/src/view${index}.tsx`),
      ...Array.from({ length: 6 }, (_, index) => `ui/e2e/flow${index}.ts`),
    ];

    expect(summarizeLayout(paths, 3)).toBe(
      ["# layout: 58 indexed files (md 40, tsx 12, ts 6)", "40\tdocs/plans/", "6\tui/e2e/", "12\tui/src/"].join("\n"),
    );
  });
});
