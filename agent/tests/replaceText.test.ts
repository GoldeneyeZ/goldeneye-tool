import { mkdir, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { FreshnessProbe } from "../src/kernel/freshness.js";
import type { NewFiles } from "../src/kernel/newFiles.js";
import { SourceReader } from "../src/kernel/sourceFiles.js";
import { globPattern } from "../src/workflows/exploreCode.js";
import type { CodeContext } from "../src/workflows/readCode.js";
import { REPLACE_MAX_LINES_SHOWN, replaceText, withLineBreaks } from "../src/workflows/replaceText.js";

let workspace: string | undefined;

async function createContext(files: Record<string, string>): Promise<CodeContext> {
  workspace = await mkdtemp(join(tmpdir(), "gcal-replace-"));
  for (const [filePath, text] of Object.entries(files)) {
    await mkdir(join(workspace, dirname(filePath)), { recursive: true });
    await writeFile(join(workspace, filePath), text);
  }
  const client = { tool: async () => ({}) };
  const paths = Object.keys(files);
  return {
    client,
    project: "demo",
    root: workspace,
    reader: new SourceReader(workspace),
    freshness: new FreshnessProbe(client),
    newFiles: { all: async () => paths, list: async () => [] } as unknown as NewFiles,
  };
}

const read = (filePath: string) => readFile(join(workspace!, filePath), "utf8");
const request = { literal: false, dryRun: false };

afterEach(async () => {
  if (workspace !== undefined) await rm(workspace, { recursive: true, force: true });
  workspace = undefined;
});

describe("replacing text", () => {
  it("rewrites every match under the given paths and prints the changed lines", async () => {
    const context = await createContext({
      "src/a.ts": 'import { native } from "./native";\r\nnative.read();\r\nconst nativeLike = 1;\r\n',
      "src/deep/b.ts": "await native.write(native.path);\n",
      "docs/guide.md": "native.read() is documented here\n",
      "assets/logo.png": "native.",
    });

    const result = await replaceText(context, {
      ...request,
      pattern: "\\bnative\\.",
      replacement: "nativeBridge.",
      paths: ["src"],
    });

    expect(result.ok).toBe(true);
    expect(result.text).toBe(
      [
        "# replaced 3 occurrences in 2 files",
        "src/a.ts\t1",
        "  2: nativeBridge.read();",
        "src/deep/b.ts\t2",
        "  1: await nativeBridge.write(nativeBridge.path);",
        // A file outside the given paths still has the old name; binary files are not counted.
        "# the pattern also matches outside these paths, unchanged (path occurrences): docs/guide.md 1",
      ].join("\n"),
    );
    // Line endings and everything outside the matches are kept.
    expect(await read("src/a.ts")).toBe(
      'import { native } from "./native";\r\nnativeBridge.read();\r\nconst nativeLike = 1;\r\n',
    );
    expect(await read("docs/guide.md")).toBe("native.read() is documented here\n");
    expect(await read("assets/logo.png")).toBe("native.");
  });

  it("supports capture groups, literal text, the whole repository and a dry run", async () => {
    const context = await createContext({
      "a.py": "from lod.pairs import canonical\nvalue = canonical(row)\n",
      "b/c.py": "price = cost * $1\n",
    });

    const groups = await replaceText(context, {
      ...request,
      pattern: "canonical\\((\\w+)\\)",
      replacement: "canonical_json($1, strict=True)",
      paths: ["."],
    });
    expect(groups.text).toContain("  2: value = canonical_json(row, strict=True)");

    const preview = await replaceText(context, {
      pattern: "$1",
      replacement: "$2",
      paths: ["b/c.py"],
      literal: true,
      dryRun: true,
    });
    expect(preview.text).toBe("# would replace 1 occurrence in 1 file\nb/c.py\t1\n  1: price = cost * $2");
    expect(await read("b/c.py")).toBe("price = cost * $1\n");

    await replaceText(context, { pattern: "$1", replacement: "$&", paths: ["b"], literal: true, dryRun: false });
    expect(await read("b/c.py")).toBe("price = cost * $&\n");
  });

  it("writes a line break for \\n in a regex replacement, and keeps it as typed inside quotes or with -F", async () => {
    const context = await createContext({
      "a.py": "from lod.pairs import canonical, load_pairs\r\nlog(done)\r\n",
      "b.py": "from lod.pairs import canonical, load_pairs\n",
      "c.py": "from lod.pairs import canonical, load_pairs\n",
    });

    await replaceText(context, {
      ...request,
      pattern: "from lod\\.pairs import canonical, (.+)",
      replacement: "from lod.pairs import $1\\nfrom lod.evaluation import\\tcanonical_json",
      paths: ["a.py", "b.py"],
    });
    expect(await read("a.py")).toBe(
      "from lod.pairs import load_pairs\r\nfrom lod.evaluation import\tcanonical_json\r\nlog(done)\r\n",
    );
    expect(await read("b.py")).toBe("from lod.pairs import load_pairs\nfrom lod.evaluation import\tcanonical_json\n");

    await replaceText(context, { ...request, pattern: "log\\(done\\)", replacement: 'log("done\\n") # a\\\\nb', paths: ["a.py"] });
    expect(await read("a.py")).toContain('log("done\\n") # a\\\\nb\r\n');

    await replaceText(context, { pattern: "canonical, ", replacement: "a\\nb, ", paths: ["c.py"], literal: true, dryRun: false });
    expect(await read("c.py")).toBe("from lod.pairs import a\\nb, load_pairs\n");
    expect(withLineBreaks("a\\nb 'c\\nd' e\\tf \\", "\n")).toBe("a\nb 'c\\nd' e\tf \\");
  });

  it("rewrites only the files a glob selects and names the others that match", async () => {
    const context = await createContext({
      "src/a.py": "value = canonical(row)\n",
      "src/data/rows.json": '{"note": "canonical(row)"}\n',
      "docs/guide.md": "call canonical(row)\n",
    });

    const result = await replaceText(context, {
      ...request,
      pattern: "canonical\\(",
      replacement: "canonical_json(",
      paths: ["src", "docs"],
      filePattern: globPattern("*.py"),
    });

    expect(result.text).toBe(
      [
        "# replaced 1 occurrence in 1 file",
        "src/a.py\t1",
        "  1: value = canonical_json(row)",
        "# the pattern also matches in files left out, unchanged (path occurrences): src/data/rows.json 1, docs/guide.md 1",
      ].join("\n"),
    );
    expect(await read("src/data/rows.json")).toBe('{"note": "canonical(row)"}\n');

    // A glob that selects no matching file: the files it left out are named.
    const none = await replaceText(context, {
      ...request,
      pattern: "canonical\\(",
      replacement: "canonical_json(",
      paths: ["src", "docs"],
      filePattern: globPattern("*.md"),
      dryRun: true,
    });
    expect(none.text).toBe("# would replace 1 occurrence in 1 file\ndocs/guide.md\t1\n  1: call canonical_json(row)\n# the pattern also matches in files left out, unchanged (path occurrences): src/data/rows.json 1");
    const nothing = await replaceText(context, {
      ...request,
      pattern: "canonical\\(",
      replacement: "canonical_json(",
      paths: ["src", "docs"],
      filePattern: globPattern("*.rs"),
    });
    expect(nothing.text).toBe(
      "# no match: canonical\\( in src docs\n# the pattern also matches in files left out, unchanged (path occurrences): src/data/rows.json 1, docs/guide.md 1",
    );
  });

  it("refuses to run without a scope and reports a pattern that matches nothing", async () => {
    const context = await createContext({ "a.ts": "one\n" });

    const unscoped = await replaceText(context, { ...request, pattern: "one", replacement: "two", paths: [] });
    expect(unscoped.ok).toBe(false);
    expect(await read("a.ts")).toBe("one\n");

    const nothing = await replaceText(context, { ...request, pattern: "three", replacement: "x", paths: ["."] });
    expect(nothing).toEqual({ ok: false, text: "# no match: three in ." });

    const invalid = await replaceText(context, { ...request, pattern: "(", replacement: "x", paths: ["."] });
    expect(invalid.ok).toBe(false);
    expect(invalid.text).toContain("# not a regular expression: (");
  });

  it("reads doubled backslashes as single ones when the pattern matches nothing", async () => {
    const context = await createContext({ "a.ts": "native.read();\nconst nativeLike = 1;\n" });

    const result = await replaceText(context, {
      ...request,
      pattern: "\\\\bnative\\\\.",
      replacement: "bridge.",
      paths: ["."],
    });

    expect(result).toEqual({
      ok: true,
      text: [
        "# no match for \\\\bnative\\\\.; read as \\bnative\\. (doubled backslashes)",
        "# replaced 1 occurrence in 1 file",
        "a.ts\t1",
        "  1: bridge.read();",
      ].join("\n"),
    });
    expect(await read("a.ts")).toBe("bridge.read();\nconst nativeLike = 1;\n");

    const literal = await replaceText(context, {
      ...request,
      literal: true,
      pattern: "\\\\bnativeLike",
      replacement: "x",
      paths: ["."],
    });
    expect(literal.ok).toBe(false);
  });

  it("keeps to the directory that exists under that path", async () => {
    const context = await createContext({ "src/a.ts": "old\n", "native/src/b.rs": "old\n" });

    await replaceText(context, { ...request, pattern: "old", replacement: "new", paths: ["src"] });
    expect(await read("src/a.ts")).toBe("new\n");
    expect(await read("native/src/b.rs")).toBe("old\n");

    // A name that is not a path from the root still finds the file anywhere.
    await replaceText(context, { ...request, pattern: "old", replacement: "new", paths: ["b.rs"] });
    expect(await read("native/src/b.rs")).toBe("new\n");
  });

  it("caps the changed lines it prints and never follows a link", async () => {
    const many = Array.from({ length: REPLACE_MAX_LINES_SHOWN + 5 }, () => "old").join("\n");
    const context = await createContext({ "many.txt": many, "outside/target.txt": "old\n" });
    let linked = true;
    try {
      await symlink(join(context.root, "outside/target.txt"), join(context.root, "link.txt"));
    } catch {
      // Creating links needs a privilege on Windows; the plain-file check is still covered below.
      linked = false;
    }
    context.newFiles = {
      all: async () => ["many.txt", ...(linked ? ["link.txt"] : []), "missing.txt"],
      list: async () => [],
    } as unknown as NewFiles;

    const result = await replaceText(context, { ...request, pattern: "old", replacement: "new", paths: ["."] });

    expect(result.text).toContain(`# replaced ${REPLACE_MAX_LINES_SHOWN + 5} occurrences in 1 file`);
    expect(result.text).toContain("# ... 5 more changed lines not shown: git diff");
    expect(await read("outside/target.txt")).toBe("old\n");
  });
});
