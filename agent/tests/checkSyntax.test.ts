import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { FreshnessProbe } from "../src/kernel/freshness.js";
import type { NewFiles } from "../src/kernel/newFiles.js";
import { SourceReader } from "../src/kernel/sourceFiles.js";
import { checkSyntax, pythonSyntaxErrors, type PythonChecker } from "../src/workflows/checkSyntax.js";
import type { CodeContext } from "../src/workflows/readCode.js";

let workspace: string | undefined;

interface Fixture {
  /** Working-tree files. */
  files: Record<string, string>;
  /** Files that differ from HEAD; default: all of them. */
  changed?: string[];
  /** Committed text of files that exist at HEAD. */
  head?: Record<string, string>;
  /** Zero-based rows with a syntax error, by file text. */
  errorRows: (text: string) => number[];
  calls?: Array<Record<string, unknown>>;
  failWith?: string;
}

async function createContext(fixture: Fixture): Promise<CodeContext> {
  workspace = await mkdtemp(join(tmpdir(), "gcal-check-"));
  for (const [filePath, text] of Object.entries(fixture.files)) {
    await mkdir(join(workspace, dirname(filePath)), { recursive: true });
    await writeFile(join(workspace, filePath), text);
  }
  const checked = (path: string, text: string | undefined) => {
    if (text === undefined) return { path, status: "unreadable", diagnostic_total: 0, diagnostics: [] };
    if (path.endsWith(".md")) return { path, status: "unsupported", diagnostic_total: 0, diagnostics: [] };
    const rows = fixture.errorRows(text);
    return {
      path,
      status: rows.length === 0 ? "clean" : "errors",
      diagnostic_total: rows.length,
      diagnostics: rows.map((row) => ({
        kind: "error",
        node_kind: "ERROR",
        span: { bytes: { start: 0, end: 1 }, start: { row, column_bytes: 0 }, end: { row, column_bytes: 1 } },
      })),
    };
  };
  const client = {
    tool: async (name: string, args: Record<string, unknown>) => {
      if (name !== "check_syntax") return {};
      fixture.calls?.push(args);
      if (fixture.failWith !== undefined) throw new Error(fixture.failWith);
      const paths = (args.paths as string[] | undefined) ?? [];
      const contents = (args.contents as Array<{ path: string; content: string }> | undefined) ?? [];
      return {
        files: [
          ...paths.map((path) => checked(path, fixture.files[path])),
          ...contents.map((item) => checked(item.path, item.content)),
        ],
      };
    },
  };
  const paths = Object.keys(fixture.files);
  return {
    client,
    project: "demo",
    root: workspace,
    reader: new SourceReader(workspace),
    freshness: new FreshnessProbe(client),
    newFiles: {
      all: async () => paths,
      list: async () => [],
      changed: async () => fixture.changed ?? paths,
      atHead: async (filePath: string) => fixture.head?.[filePath],
    } as unknown as NewFiles,
  };
}

afterEach(async () => {
  if (workspace !== undefined) await rm(workspace, { recursive: true, force: true });
  workspace = undefined;
});

/** No Python interpreter: only the backend's grammars decide. */
const GRAMMAR_ONLY = { python: null };

/** A line that holds `BROKEN` does not parse. */
const brokenLines = (text: string) =>
  text.split("\n").flatMap((line, row) => (line.includes("BROKEN") ? [row] : []));

describe("checking syntax", () => {
  it("names the lines that do not parse in the files that differ from HEAD", async () => {
    const calls: Array<Record<string, unknown>> = [];
    const context = await createContext({
      files: {
        "src/pilot.py": "def run():\n  ok()\n    BROKEN indent\n",
        "src/fine.py": "def fine():\n  pass\n",
        "src/untouched.py": "BROKEN but not changed\n",
        "docs/notes.md": "text\n",
        "assets/logo.png": "BROKEN",
      },
      changed: ["src/pilot.py", "src/fine.py", "docs/notes.md", "assets/logo.png", "src/gone.py"],
      head: { "src/pilot.py": "def run():\n  ok()\n" },
      errorRows: brokenLines,
      calls,
    });

    const result = await checkSyntax(context, { paths: [] }, GRAMMAR_ONLY);

    expect(result).toEqual({
      ok: true,
      text: [
        "# syntax check: 1 of 2 changed files do not parse",
        "src/pilot.py",
        "  3: BROKEN indent",
        "# 1 could not be read: src/gone.py",
        "# 1 not checked (no grammar): docs/notes.md",
      ].join("\n"),
    });
    // The text is sent, never a path; binary and missing files are not sent, and the
    // committed text is checked only for files with errors.
    expect(calls).toEqual([
      {
        contents: [
          { path: "src/pilot.py", content: "def run():\n  ok()\n    BROKEN indent\n" },
          { path: "src/fine.py", content: "def fine():\n  pass\n" },
          { path: "docs/notes.md", content: "text\n" },
        ],
      },
      { contents: [{ path: "src/pilot.py", content: "def run():\n  ok()\n" }] },
    ]);
  });

  it("lets a Python interpreter decide for Python files when there is one", async () => {
    const fixture = {
      // The grammar accepts both Python files; only the interpreter sees the lost indent.
      files: { "src/pilot.py": "def run():\nreturn 1\n", "src/fine.py": "x = 1\n", "src/app.ts": "BROKEN\n" },
      head: { "src/pilot.py": "def run():\n    return 1\n" },
      errorRows: brokenLines,
    };
    const asked: string[][] = [];
    const python: PythonChecker = async (sources) => {
      asked.push(sources.map((source) => source.path));
      return new Map(
        sources
          .filter((source) => source.content.includes("\nreturn"))
          .map((source) => [source.path, { line: 2, message: "expected an indented block" }]),
      );
    };

    const result = await checkSyntax(await createContext(fixture), { paths: [] }, { python });

    expect(result.text).toBe(
      [
        "# syntax check: 2 of 3 changed files do not parse",
        "src/pilot.py",
        "  2: return 1\texpected an indented block",
        "src/app.ts",
        "  1: BROKEN",
      ].join("\n"),
    );
    // Python files only, then the committed version of the one with an error.
    expect(asked).toEqual([["src/pilot.py", "src/fine.py"], ["src/pilot.py"]]);

    // No interpreter on this machine: the grammar's verdict stands.
    const none = await checkSyntax(
      await createContext(fixture),
      { paths: [] },
      { python: async () => undefined },
    );
    expect(none.text).toBe(
      ["# syntax check: 1 of 3 changed files do not parse", "src/app.ts", "  1: BROKEN"].join("\n"),
    );
  });

  it("finds an indentation error with the interpreter of this machine, when it has one", async () => {
    const errors = await pythonSyntaxErrors([
      { path: "ok.py", content: "def run():\n    return 1\n" },
      { path: "bad.py", content: "def run():\n    x = 1\n        return x\n" },
    ]);

    if (errors === undefined) return; // no Python here; the injected checker above covers the logic
    expect([...errors.keys()]).toEqual(["bad.py"]);
    expect(errors.get("bad.py")).toEqual({ line: 3, message: "unexpected indent" });
  });

  it("keeps every backend request below the daemon's request size", async () => {
    const calls: Array<Record<string, unknown>> = [];
    const big = `${"x = 1\n".repeat(50_000)}`; // 300 KB each
    const context = await createContext({
      files: { "a.ts": big, "b.ts": big, "c.ts": `${big}BROKEN\n`, "d.ts": "ok\n" },
      errorRows: brokenLines,
      calls,
    });

    const result = await checkSyntax(context, { paths: [] }, GRAMMAR_ONLY);

    expect(result.text.split("\n").slice(0, 2)).toEqual([
      "# syntax check: 1 of 4 changed files do not parse",
      "c.ts",
    ]);
    // Each large file is about 350 KB on the wire, so two do not share a request.
    // c.ts is new: nothing is compared with HEAD.
    expect(calls.map((call) => (call.contents as unknown[]).length)).toEqual([1, 1, 2]);
    for (const call of calls) {
      expect(Buffer.byteLength(JSON.stringify(call))).toBeLessThan(1024 * 1024);
    }
  });

  it("does not blame the change for diagnostics the committed file already has", async () => {
    const context = await createContext({
      files: {
        "src/modern.ts": "const a = 1;\nBROKEN new syntax the grammar lacks\nconst b = 2;\n",
        "src/worse.ts": "BROKEN old\nBROKEN new\n",
        "src/new.ts": "BROKEN in a new file\n",
      },
      head: {
        "src/modern.ts": "BROKEN new syntax the grammar lacks\n",
        "src/worse.ts": "BROKEN old\n",
      },
      errorRows: brokenLines,
    });

    const result = await checkSyntax(context, { paths: [] }, GRAMMAR_ONLY);

    expect(result.text).toBe(
      [
        "# syntax check: 2 of 3 changed files do not parse",
        "src/worse.ts",
        "  1: BROKEN old",
        "  2: BROKEN new",
        "src/new.ts",
        "  1: BROKEN in a new file",
        "# 1 with the same diagnostics as at HEAD, not caused by the change: src/modern.ts",
      ].join("\n"),
    );
  });

  it("checks given paths, and says so when nothing is wrong or nothing changed", async () => {
    const context = await createContext({
      files: { "src/a.py": "ok\n", "src/deep/b.py": "ok\n", "other/c.py": "BROKEN\n" },
      changed: [],
      errorRows: brokenLines,
    });

    expect((await checkSyntax(context, { paths: [] }, GRAMMAR_ONLY)).text).toBe("# syntax check: no file differs from HEAD");
    expect((await checkSyntax(context, { paths: ["src"] }, GRAMMAR_ONLY)).text).toBe("# syntax check: 2 files parse");
    expect((await checkSyntax(context, { paths: ["."] }, GRAMMAR_ONLY)).text).toBe(
      ["# syntax check: 1 of 3 files do not parse", "other/c.py", "  1: BROKEN"].join("\n"),
    );
    expect((await checkSyntax(context, { paths: ["missing"] }, GRAMMAR_ONLY)).text).toBe(
      "# syntax check: no file under these paths",
    );
  });

  it("reports a backend without the tool, and passes other failures on", async () => {
    const old = await createContext({
      files: { "a.py": "ok\n" },
      errorRows: brokenLines,
      failWith: "MCP tool error: Unknown tool: check_syntax",
    });
    expect(await checkSyntax(old, { paths: [] }, GRAMMAR_ONLY)).toEqual({
      ok: false,
      text: "# gcal check needs a Goldeneye build that has the check_syntax tool",
    });

    const down = await createContext({ files: { "a.py": "ok\n" }, errorRows: brokenLines, failWith: "backend exited" });
    await expect(checkSyntax(down, { paths: [] }, GRAMMAR_ONLY)).rejects.toThrow("backend exited");
  });
});
