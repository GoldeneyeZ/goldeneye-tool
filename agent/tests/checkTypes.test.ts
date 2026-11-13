import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { FreshnessProbe } from "../src/kernel/freshness.js";
import type { NewFiles } from "../src/kernel/newFiles.js";
import { SourceReader } from "../src/kernel/sourceFiles.js";
import { checkTypes, parseTypeScriptErrors } from "../src/workflows/checkTypes.js";
import type { CodeContext } from "../src/workflows/readCode.js";

let workspace: string | undefined;

afterEach(async () => {
  if (workspace !== undefined) await rm(workspace, { recursive: true, force: true });
  workspace = undefined;
});

async function createContext(changed: string[], withCompiler: boolean): Promise<CodeContext> {
  workspace = await mkdtemp(join(tmpdir(), "gcal-types-"));
  if (withCompiler) {
    await mkdir(join(workspace, "node_modules", "typescript", "bin"), { recursive: true });
    await writeFile(join(workspace, "node_modules", "typescript", "bin", "tsc"), "");
    await writeFile(join(workspace, "tsconfig.json"), "{}");
  }
  const client = { tool: async () => ({}) };
  return {
    client,
    project: "demo",
    root: workspace,
    reader: new SourceReader(workspace),
    freshness: new FreshnessProbe(client),
    newFiles: { changed: async () => changed } as unknown as NewFiles,
  };
}

const OUTPUT = [
  "src/lib/fuzzy.ts(63,24): error TS2339: Property 'length' does not exist on type '{ chars: string[]; indices: number[]; }'.",
  "src\\app\\App.tsx(12,3): error TS2304: Cannot find name 'native'.",
  "Found 2 errors in 2 files.",
].join("\r\n");

describe("type check", () => {
  it("reads the compiler's rows", () => {
    expect(parseTypeScriptErrors(OUTPUT)).toEqual([
      {
        filePath: "src/lib/fuzzy.ts",
        line: 63,
        code: "TS2339",
        message: "Property 'length' does not exist on type '{ chars: string[]; indices: number[]; }'.",
      },
      { filePath: "src/app/App.tsx", line: 12, code: "TS2304", message: "Cannot find name 'native'." },
    ]);
  });

  it("reports the errors HEAD does not have, the changed files first", async () => {
    const context = await createContext(["src/lib/fuzzy.ts"], true);
    const calls: string[][] = [];
    const run = async (_command: string, args: string[]) => {
      calls.push(args.slice(1));
      return OUTPUT;
    };

    // HEAD type-checks clean: both errors are new.
    const fresh = await checkTypes(context, run, async () => "");
    expect(calls).toEqual([["--noEmit", "--pretty", "false", "-p", "."]]);
    expect(fresh.text).toBe(
      [
        "# type check (tsc --noEmit): 2 new errors",
        "  src/lib/fuzzy.ts:63\tTS2339\tProperty 'length' does not exist on type '{ chars: string[]; indices: number[]; }'.",
        "# 1 of them in files the change did not touch:",
        "  src/app/App.tsx:12\tTS2304\tCannot find name 'native'.",
      ].join("\n"),
    );

    // HEAD already has the error of App.tsx, on another line.
    const older = "src/app/App.tsx(9,3): error TS2304: Cannot find name 'native'.\n";
    expect((await checkTypes(context, run, async () => older)).text).toBe(
      [
        "# type check (tsc --noEmit): 1 new error",
        "  src/lib/fuzzy.ts:63\tTS2339\tProperty 'length' does not exist on type '{ chars: string[]; indices: number[]; }'.",
        "# 1 more that HEAD has too are left out",
      ].join("\n"),
    );
    expect((await checkTypes(context, run, async () => OUTPUT)).text).toBe(
      "# type check (tsc --noEmit): no new error (2 that HEAD has too)",
    );
    // No comparison possible: everything is shown, with the caveat.
    expect((await checkTypes(context, run, async () => undefined)).text).toContain(
      "# HEAD could not be type-checked for comparison: some of these may be older than the change",
    );

    expect((await checkTypes(context, async () => "")).text).toBe("# type check (tsc --noEmit): no error");
  });

  it("says why it did not run", async () => {
    const never = async () => {
      throw new Error("the compiler must not be run");
    };
    expect((await checkTypes(await createContext(["src/a.py"], true), never)).text).toBe(
      "# type check: no TypeScript file differs from HEAD (only TypeScript is type-checked)",
    );
    expect((await checkTypes(await createContext(["src/a.ts"], false), never)).text).toBe(
      "# type check: not run (needs tsconfig.json at the project root and typescript in node_modules)",
    );
  });
});
