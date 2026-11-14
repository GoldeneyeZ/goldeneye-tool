import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { droppedImports } from "../src/kernel/droppedImports.js";
import { FreshnessProbe } from "../src/kernel/freshness.js";
import type { NewFiles } from "../src/kernel/newFiles.js";
import { SourceReader } from "../src/kernel/sourceFiles.js";
import { removedImportsStillUsed } from "../src/workflows/checkImports.js";
import type { CodeContext } from "../src/workflows/readCode.js";

const text = (...lines: string[]): string => lines.join("\n");
const names = (filePath: string, before: string, after: string) =>
  droppedImports(filePath, before, after).map((dropped) => `${dropped.name}@${dropped.uses.map((use) => use.line).join(",")}`);

describe("droppedImports", () => {
  it("names a module whose import left with a function that moved, and that the file still uses", () => {
    const before = text(
      "import hashlib",
      "import json",
      "",
      "def canonical(value):",
      "    return json.dumps(value)",
      "",
      "def load(path):",
      "    return json.loads(path.read_text())",
    );
    const after = text(
      "import hashlib",
      "",
      "from lib.evaluation import canonical_json",
      "",
      "def load(path):",
      "    return json.loads(path.read_text())",
    );
    expect(droppedImports("src/pairs.py", before, after)).toEqual([
      { name: "json", statement: "import json", uses: [{ line: 6, text: "    return json.loads(path.read_text())" }] },
    ]);
  });

  it("does not read prose, a keyword argument or a member of something else as a use", () => {
    const before = text("import json", "import os.path", "", "def send(session, body):", "    return json.dumps(body)");
    const after = text(
      '"""Sends the json body.',
      "",
      "The json module is not needed here.",
      '"""',
      "",
      "def send(session, body):",
      "    # json is encoded by the session",
      '    label = "json"',
      "    return session.post(url, json=body, data=self.json)",
    );
    expect(names("src/send.py", before, after)).toEqual([]);
  });

  it("accepts an import that moved, was renamed with an alias, or whose name the file now defines", () => {
    const before = text("import json", "from lib.pairs import canonical, load_pairs", "", "print(canonical(json.loads(load_pairs())))");
    const moved = text("from lib.pairs import (", "    load_pairs,", ")", "", "def run():", "    import json", "    canonical = str", "    print(canonical(json.loads(load_pairs())))");
    expect(names("src/run.py", before, moved)).toEqual([]);
    const lost = text("from lib.pairs import (", "    load_pairs,", ")", "", "print(canonical(json.loads(load_pairs())))");
    expect(names("src/run.py", before, lost)).toEqual(["json@5", "canonical@5"]);
    const star = text("from lib.pairs import *", "", "print(canonical(json.loads(load_pairs())))");
    expect(names("src/run.py", before, star)).toEqual([]);
  });

  it("reads the imports of TypeScript, Java and Rust", () => {
    const script = text(
      'import fs, { readFile as read, type Stats } from "node:fs";',
      'import * as path from "node:path";',
      'import {',
      "  native,",
      '} from "./native";',
      'const { join } = require("node:path");',
      "",
      "export const run = (stats: Stats) => read(path.join(fs.cwd(), native.root, join(a)));",
    );
    const scriptAfter = text(
      'import { nativeBridge } from "@/lib/nativeBridge";',
      "",
      "const options = { native: true, path };",
      "export const run = (stats: Stats) => read(path.join(fs.cwd(), native.root, join(a)));",
    );
    expect(names("src/run.ts", script, scriptAfter)).toEqual(["read@4", "Stats@4", "fs@4", "path@3,4", "native@4", "join@4"]);

    const java = text("import java.util.List;", "import static java.util.Objects.requireNonNull;", "class A { List<String> a = requireNonNull(b); }");
    expect(names("src/A.java", java, text("class A { List<String> a = requireNonNull(b); }"))).toEqual(["List@1", "requireNonNull@1"]);
    expect(names("src/A.java", java, text("import java.util.*;", "class A { List<String> a; }"))).toEqual([]);

    const rust = text("use std::{", "    fmt::{self, Write as _},", "    path::{Path, PathBuf},", "};", "fn f(p: &Path) -> PathBuf { fmt::format(p) }");
    const rustAfter = text("use std::path::PathBuf;", "fn f(p: &Path) -> PathBuf { fmt::format(std::path::Path::new(p)) }");
    expect(names("src/lib.rs", rust, rustAfter)).toEqual(["fmt@2", "Path@2"]);
  });

  it("says nothing for a language whose imports it does not read", () => {
    expect(names("src/main.go", 'import "fmt"\nfunc main() { fmt.Println() }', "func main() { fmt.Println() }")).toEqual([]);
  });
});

let workspace: string | undefined;

afterEach(async () => {
  if (workspace !== undefined) await rm(workspace, { recursive: true, force: true });
  workspace = undefined;
});

async function createContext(files: Record<string, string>, head: Record<string, string>): Promise<CodeContext> {
  workspace = await mkdtemp(join(tmpdir(), "gcal-imports-"));
  for (const [filePath, content] of Object.entries(files)) {
    await mkdir(join(workspace, dirname(filePath)), { recursive: true });
    await writeFile(join(workspace, filePath), content);
  }
  const client = { tool: async () => ({}) };
  return {
    client,
    project: "demo",
    root: workspace,
    reader: new SourceReader(workspace),
    freshness: new FreshnessProbe(client),
    newFiles: {
      all: async () => Object.keys(files),
      list: async () => Object.keys(files).filter((filePath) => head[filePath] === undefined),
      changed: async () => Object.keys(files).filter((filePath) => files[filePath] !== head[filePath]),
      deleted: async () => [],
      atHead: async (filePath: string) => head[filePath],
    } as unknown as NewFiles,
  };
}

describe("removedImportsStillUsed", () => {
  it("prints the files the change edited whose removed import is still used", async () => {
    const head = {
      "src/pairs.py": text("import json", "", "def load(path):", "    a = json.loads(path)", "    b = json.loads(a)", "    c = json.loads(b)", "    return json.dumps(c)"),
      "src/same.py": text("import json", "print(json.dumps(1))"),
    };
    const context = await createContext(
      {
        "src/pairs.py": text("def load(path):", "    a = json.loads(path)", "    b = json.loads(a)", "    c = json.loads(b)", "    return json.dumps(c)"),
        "src/same.py": head["src/same.py"],
        "src/new.py": "print(json.dumps(1))",
      },
      head,
    );
    expect(await removedImportsStillUsed(context)).toEqual([
      "# an import the change removed, and a name the file still uses:",
      "src/pairs.py: `import json` is gone, `json` is still used",
      "  2\t    a = json.loads(path)",
      "  3\t    b = json.loads(a)",
      "  4\t    c = json.loads(b)",
      "  ... 1 more lines",
    ]);
  });

  it("prints nothing when every removed import is unused", async () => {
    const head = { "src/a.py": text("import json", "print(1)") };
    expect(await removedImportsStillUsed(await createContext({ "src/a.py": "print(1)" }, head))).toEqual([]);
  });
});
