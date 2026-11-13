import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import type { GcalBackendClient } from "../src/domain/GcalBackendClient.js";
import type { GraphNode } from "../src/kernel/codeGraph.js";
import { FreshnessProbe } from "../src/kernel/freshness.js";
import type { NewFiles } from "../src/kernel/newFiles.js";
import { SourceReader } from "../src/kernel/sourceFiles.js";
import { leftBehind, unmatchedConstructors } from "../src/workflows/checkReferences.js";
import type { CodeContext } from "../src/workflows/readCode.js";

const PROJECT = "demo";
let workspace: string | undefined;

afterEach(async () => {
  if (workspace !== undefined) await rm(workspace, { recursive: true, force: true });
  workspace = undefined;
});

function node(filePath: string, label: string, rest: string[], startLine: number, endLine: number): GraphNode {
  return {
    qualifiedName: [PROJECT, ...filePath.replace(/\.[a-z]+$/, "").split("/"), ...rest].join("."),
    name: rest[rest.length - 1],
    label,
    filePath,
    startLine,
    endLine,
  };
}

interface Fixture {
  /** The working tree. */
  files: Record<string, string>;
  /** What HEAD holds for the files that are not new. */
  head: Record<string, string>;
  /** Files that differ from HEAD and exist. */
  changed: string[];
  /** Files HEAD has and the working tree does not. */
  deleted?: string[];
  /** Files that are new. */
  added?: string[];
  /** The index, built at HEAD. */
  nodes: GraphNode[];
  /** Files the call graph says call a symbol, by qualified name. */
  callers: Record<string, string[]>;
  /** Symbols whose text names a word, with the lines that do. */
  mentions?: Array<{ symbol: GraphNode; lines: number[] }>;
}

async function createFixture(fixture: Fixture): Promise<{ context: CodeContext; backend: GcalBackendClient }> {
  workspace = await mkdtemp(join(tmpdir(), "gcal-left-"));
  for (const [filePath, text] of Object.entries(fixture.files)) {
    await mkdir(join(workspace, dirname(filePath)), { recursive: true });
    await writeFile(join(workspace, filePath), text);
  }
  const client = {
    tool: async (name: string, args: Record<string, unknown>) => {
      if (name === "query_graph") {
        const filePath = /'([^']*)'/.exec(String(args.query))?.[1] ?? "";
        return {
          rows: fixture.nodes
            .filter((item) => item.filePath === filePath)
            .map((item) => [item.qualifiedName, item.name, [item.label], item.filePath, item.startLine, item.endLine]),
        };
      }
      if (name === "search_graph") {
        const wanted = new RegExp(String(args.name_pattern));
        return {
          results: fixture.nodes
            .filter((item) => wanted.test(item.name))
            .map((item) => ({
              qualified_name: item.qualifiedName,
              name: item.name,
              label: item.label,
              file_path: item.filePath,
              start_line: item.startLine,
              end_line: item.endLine,
            })),
        };
      }
      if (name === "search_code") {
        return {
          results: (fixture.mentions ?? []).map(({ symbol, lines }) => ({
            file: symbol.filePath,
            label: symbol.label,
            node: symbol.name,
            qualified_name: symbol.qualifiedName,
            start_line: symbol.startLine,
            end_line: symbol.endLine,
            match_lines: lines,
          })),
          total_results: 0,
        };
      }
      return {};
    },
  };
  const context: CodeContext = {
    client,
    project: PROJECT,
    root: workspace,
    reader: new SourceReader(workspace),
    freshness: new FreshnessProbe(client),
    newFiles: {
      all: async () => Object.keys(fixture.files),
      list: async () => fixture.added ?? [],
      changed: async () => fixture.changed,
      deleted: async () => fixture.deleted ?? [],
      atHead: async (filePath: string) => fixture.head[filePath],
    } as unknown as NewFiles,
  };
  const backend = {
    callers: async (qualifiedName: string) =>
      (fixture.callers[qualifiedName] ?? []).map((filePath) => ({
        sourceQualifiedName: `${filePath}#caller`,
        targetQualifiedName: qualifiedName,
        relatedQualifiedName: `${filePath}#caller`,
        hop: 1,
        filePath,
        line: 1,
      })),
  } as unknown as GcalBackendClient;
  return { context, backend };
}

const PATH_AT_HEAD = [
  "pub struct Checked(PathBuf);",
  "impl Checked {",
  "    pub fn as_path(&self) -> &Path {",
  "        &self.0",
  "    }",
  "}",
  "impl AsRef<Path> for Checked {",
  "    fn as_ref(&self) -> &Path {",
  "        self.as_path()",
  "    }",
  "}",
].join("\n");
const AUTH = ["pub fn revalidate(&self) -> Checked {", "    Checked(self.0.clone())", "}"].join("\n");
const checked = node("src/path.rs", "Struct", ["Checked"], 1, 1);
const asPath = node("src/path.rs", "Method", ["Checked", "as_path"], 3, 5);
const asRef = node("src/path.rs", "Method", ["Checked", "as_ref"], 8, 10);
const revalidate = node("src/auth.rs", "Function", ["revalidate"], 1, 3);

describe("what a change left behind", () => {
  it("names the callers that still use a renamed method, and only those that have its type at hand", async () => {
    const { context, backend } = await createFixture({
      files: {
        // Renamed, with one use of the old name left in the same file.
        "src/path.rs": PATH_AT_HEAD.replace("pub fn as_path", "pub fn destination"),
        "src/auth.rs": AUTH,
        // Never names the type: the value comes from a function whose signature does.
        "src/use.rs": "let checked = authorized.revalidate();\nread(checked.as_path());\n",
        // Another `as_path`, on a standard path buffer.
        "src/other.rs": "let parent = buffer.as_path();\n",
        "src/done.rs": "let checked: Checked = make();\nread(checked.destination());\n",
      },
      head: { "src/path.rs": PATH_AT_HEAD },
      changed: ["src/path.rs", "src/done.rs"],
      nodes: [checked, asPath, asRef, revalidate],
      callers: { [asPath.qualifiedName]: ["src/path.rs", "src/use.rs", "src/other.rs", "src/done.rs"] },
      mentions: [{ symbol: revalidate, lines: [1, 2] }],
    });

    expect(await leftBehind(context, backend)).toEqual([
      "# removed or renamed since the index, and still used (from the call graph):",
      "Checked.as_path (Method, was in src/path.rs) is no longer declared; its callers still name it:",
      "  src/path.rs:9\t        self.as_path()",
      "  src/use.rs:2\tread(checked.as_path());",
    ]);
  });

  it("reports nothing when the callers were updated, when the symbol moved, or when it never was a declaration", async () => {
    const renamed = await createFixture({
      files: {
        "src/path.rs": PATH_AT_HEAD.replaceAll("as_path", "destination"),
        "src/use.rs": "let checked = authorized.revalidate();\nread(checked.destination());\n",
      },
      head: { "src/path.rs": PATH_AT_HEAD },
      changed: ["src/path.rs", "src/use.rs"],
      nodes: [checked, asPath, asRef],
      callers: { [asPath.qualifiedName]: ["src/path.rs", "src/use.rs"] },
    });
    expect(await leftBehind(renamed.context, renamed.backend)).toEqual([]);

    // The function went to a new file under the same name: its callers are right to keep calling it.
    const helper = node("src/util.py", "Function", ["canonical"], 1, 2);
    const moved = await createFixture({
      files: {
        "src/util.py": "import json\n",
        "src/text.py": "def canonical(value):\n    return value\n",
        "src/main.py": "from text import canonical\nprint(canonical(1))\n",
      },
      head: { "src/util.py": "def canonical(value):\n    return value\n" },
      changed: ["src/util.py", "src/text.py", "src/main.py"],
      added: ["src/text.py"],
      nodes: [helper],
      callers: { [helper.qualifiedName]: ["src/main.py"] },
    });
    expect(await leftBehind(moved.context, moved.backend)).toEqual([]);

    // The index names a lambda after what it mentions; that is no declaration to lose.
    const lambda = node("src/Config.java", "Function", ["Config", "chain", "csrf"], 3, 3);
    const config = "class Config {\n  Chain chain(Http http) {\n    return http.csrf(csrf -> csrf.disable()).build();\n  }\n}\n";
    const edited = await createFixture({
      files: { "src/Config.java": config.replace("build()", "cors().build()") },
      head: { "src/Config.java": config },
      changed: ["src/Config.java"],
      nodes: [lambda],
      callers: { [lambda.qualifiedName]: ["src/Config.java"] },
    });
    expect(await leftBehind(edited.context, edited.backend)).toEqual([]);
  });

  it("names the files that still import a deleted file", async () => {
    const { context, backend } = await createFixture({
      files: {
        "src/lib/bridge.ts": "export const bridge = {};\n",
        "src/app.ts": 'import { native } from "./lib/native";\nnative.open();\n',
        "src/ok.ts": 'import { bridge } from "./lib/bridge";\nbridge.open();\n',
      },
      head: { "src/lib/native.ts": "export const native = {};\n" },
      changed: ["src/lib/bridge.ts", "src/ok.ts"],
      deleted: ["src/lib/native.ts"],
      added: ["src/lib/bridge.ts"],
      nodes: [],
      callers: {},
    });

    const lines = await leftBehind(context, backend);
    expect(lines[0]).toBe("# src/lib/native.ts was deleted and is still imported:");
    expect(lines[1]).toContain("# importers of src/lib/native.ts (no such file now): 1 files");
    expect(lines[1]).toContain('src/app.ts\n  1\timport { native } from "./lib/native";');
  });

  it("names a constructor call that no declared constructor of the class takes", async () => {
    const values = [
      "public class Values {",
      "  public Values() {}",
      "  public Values(Map<String, Object> original) {}",
      "  public Values(List<Value> list,",
      "      boolean copy) {}",
      "}",
    ].join("\n");
    const test = [
      "class ValuesTests {",
      "  void adds() {",
      '    Values one = new Values(Map.of("a", "b"));',
      '    Values wrong = new Values("name", "value", "more");',
      // The right number of arguments is all this check can see.
      '    Values typed = new Values("name", "value");',
      "    Dto dto = new Dto(1, 2);",
      "    Other other = new Other(1, 2, 3);",
      "  }",
      "}",
    ].join("\n");
    const { context } = await createFixture({
      files: {
        "src/Values.java": values,
        // Constructors written by an annotation processor are not in the source.
        "src/Dto.java": "@AllArgsConstructor\npublic class Dto {\n  public Dto() {}\n}\n",
        "test/ValuesTests.java": test,
      },
      // The line with `new Values(Map.of(..))` was there before the change.
      head: { "test/ValuesTests.java": test.split("\n").filter((line) => !/wrong|typed|Dto dto|Other other/.test(line)).join("\n") },
      changed: ["test/ValuesTests.java"],
      nodes: [node("src/Values.java", "Class", ["Values"], 1, 6), node("src/Dto.java", "Class", ["Dto"], 2, 4)],
      callers: {},
    });

    expect(await unmatchedConstructors(context)).toEqual([
      "# calls that match no declared constructor:",
      "  test/ValuesTests.java:4\tnew Values(...) with 3 arguments; Values declares constructors with 0, 1, 2 (src/Values.java:2,3,4)",
    ]);
  });
});
