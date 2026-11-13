import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { mkdtemp, rm, rmdir, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { normalizePath } from "../kernel/locator.js";
import type { CodeContext, ReadResult } from "./readCode.js";

// A change can parse and still not compile: a value that became an object and
// is still asked for its `length`. Only the project's own compiler sees that.
// `gcal check --types` runs it when the project has one installed and reports
// what it says about the files the change touched; the rest is counted, since
// an error elsewhere may be older than the change.

const TYPES_TIMEOUT_MS = 180_000;
const TYPES_ROWS_CHANGED = 20;
const TYPES_ROWS_OTHER = 5;
const TYPES_MESSAGE_MAX_CHARS = 200;
const TYPESCRIPT_FILE = /\.(tsx?|mts|cts)$/;

export interface TypeError {
  filePath: string;
  line: number;
  code: string;
  message: string;
}

/** Runs a compiler and returns its combined output, or undefined when it could not be run. */
export type CompilerRunner = (command: string, args: string[], cwd: string) => Promise<string | undefined>;

/**
 * Type-checks the project as HEAD has it and returns the compiler's output, or
 * undefined when that cannot be done. The errors HEAD already has are not the
 * change's.
 */
export type BaselineRunner = (root: string, compilerArgs: string[], run: CompilerRunner) => Promise<string | undefined>;

/** Pipes `git archive HEAD` into `tar`: HEAD's files, without touching the repository. */
function exportHead(root: string, target: string): Promise<boolean> {
  return new Promise((resolve) => {
    const archive = spawn("git", ["-C", root, "archive", "--format=tar", "HEAD"], { windowsHide: true });
    const extract = spawn("tar", ["-x", "-f", "-", "-C", target], { windowsHide: true });
    archive.stdout.pipe(extract.stdin);
    let failed = false;
    archive.once("error", () => (failed = true));
    extract.once("error", () => resolve(false));
    extract.stdin.once("error", () => (failed = true));
    extract.once("close", (code) => resolve(!failed && code === 0));
  });
}

const headBaseline: BaselineRunner = async (root, compilerArgs, run) => {
  const copy = await mkdtemp(join(tmpdir(), "gcal-head-"));
  const link = join(copy, "node_modules");
  try {
    if (!(await exportHead(root, copy))) return undefined;
    // The copy uses the installed dependencies through a link, never a copy of them.
    await symlink(join(root, "node_modules"), link, "junction");
    return await run(process.execPath, compilerArgs, copy);
  } catch {
    return undefined;
  } finally {
    // The link first and on its own: removing it must never reach what it points to.
    await rmdir(link).catch(() => undefined);
    if (!existsSync(link)) await rm(copy, { recursive: true, force: true }).catch(() => undefined);
  }
};

const runCompiler: CompilerRunner = (command, args, cwd) =>
  new Promise((resolve) => {
    let output = "";
    const child = spawn(command, args, { cwd, windowsHide: true });
    const timer = setTimeout(() => child.kill(), TYPES_TIMEOUT_MS);
    child.stdout.on("data", (chunk: Buffer) => (output += chunk.toString("utf8")));
    child.stderr.on("data", (chunk: Buffer) => (output += chunk.toString("utf8")));
    child.once("error", () => {
      clearTimeout(timer);
      resolve(undefined);
    });
    child.once("close", () => {
      clearTimeout(timer);
      resolve(output);
    });
  });

/** `path(line,col): error TS2339: message`, as `tsc --pretty false` prints it. */
export function parseTypeScriptErrors(output: string): TypeError[] {
  const errors: TypeError[] = [];
  for (const line of output.split(/\r?\n/)) {
    const match = /^(.+?)\((\d+),\d+\): error (TS\d+): (.*)$/.exec(line);
    if (match === null) continue;
    errors.push({ filePath: normalizePath(match[1]), line: Number(match[2]), code: match[3], message: match[4] });
  }
  return errors;
}

function row(error: TypeError): string {
  const message =
    error.message.length > TYPES_MESSAGE_MAX_CHARS
      ? `${error.message.slice(0, TYPES_MESSAGE_MAX_CHARS - 1)}…`
      : error.message;
  return `  ${error.filePath}:${error.line}\t${error.code}\t${message}`;
}

/**
 * The type errors of the project after the change. TypeScript only: the
 * compiler in the project's `node_modules`, with the project's `tsconfig.json`.
 */
export async function checkTypes(
  context: CodeContext,
  run: CompilerRunner = runCompiler,
  baseline: BaselineRunner = headBaseline,
): Promise<ReadResult> {
  if (context.newFiles === undefined) return { ok: false, text: "# type check needs a git working tree" };
  const changed = await context.newFiles.changed();
  if (!changed.some((filePath) => TYPESCRIPT_FILE.test(filePath))) {
    return { ok: true, text: "# type check: no TypeScript file differs from HEAD (only TypeScript is type-checked)" };
  }
  const compiler = join(context.root, "node_modules", "typescript", "bin", "tsc");
  if (!existsSync(join(context.root, "tsconfig.json")) || !existsSync(compiler)) {
    return {
      ok: true,
      text: "# type check: not run (needs tsconfig.json at the project root and typescript in node_modules)",
    };
  }
  const compilerArgs = [compiler, "--noEmit", "--pretty", "false", "-p", "."];
  const output = await run(process.execPath, compilerArgs, context.root);
  if (output === undefined) return { ok: false, text: "# type check: the TypeScript compiler could not be run" };

  const all = parseTypeScriptErrors(output);
  if (all.length === 0) return { ok: true, text: "# type check (tsc --noEmit): no error" };

  // What HEAD already has is not the change's. An error is the same one when
  // file, code and message agree; its line may have moved. Each old error
  // cancels one new one, so a second error of the same kind still shows.
  const before = await baseline(context.root, compilerArgs, run);
  const old = new Map<string, number>();
  const key = (error: TypeError) => `${error.filePath}\u0000${error.code}\u0000${error.message}`;
  for (const error of before === undefined ? [] : parseTypeScriptErrors(before)) {
    old.set(key(error), (old.get(key(error)) ?? 0) + 1);
  }
  const errors = all.filter((error) => {
    const left = old.get(key(error)) ?? 0;
    if (left === 0) return true;
    old.set(key(error), left - 1);
    return false;
  });
  const older = all.length - errors.length;
  const olderNote =
    before === undefined
      ? "# HEAD could not be type-checked for comparison: some of these may be older than the change"
      : older > 0
        ? `# ${older} more that HEAD has too are left out`
        : undefined;
  if (errors.length === 0) {
    return { ok: true, text: `# type check (tsc --noEmit): no new error (${older} that HEAD has too)` };
  }

  const touched = new Set(changed);
  const mine = errors.filter((error) => touched.has(error.filePath));
  const other = errors.filter((error) => !touched.has(error.filePath));
  const lines = [`# type check (tsc --noEmit): ${errors.length} new error${errors.length === 1 ? "" : "s"}`];
  lines.push(...mine.slice(0, TYPES_ROWS_CHANGED).map(row));
  if (mine.length > TYPES_ROWS_CHANGED) lines.push(`  ... ${mine.length - TYPES_ROWS_CHANGED} more in the changed files`);
  if (other.length > 0) {
    lines.push(`# ${other.length} of them in files the change did not touch:`);
    lines.push(...other.slice(0, TYPES_ROWS_OTHER).map(row));
    if (other.length > TYPES_ROWS_OTHER) lines.push(`  ... ${other.length - TYPES_ROWS_OTHER} more`);
  }
  if (olderNote !== undefined) lines.push(olderNote);
  return { ok: true, text: lines.join("\n") };
}
