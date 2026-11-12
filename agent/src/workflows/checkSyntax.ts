import { spawn } from "node:child_process";
import { readFile, stat } from "node:fs/promises";
import { join } from "node:path";
import { BINARY_FILE, pathsPattern, separatePaths } from "./exploreCode.js";
import type { CodeContext, ReadResult } from "./readCode.js";

// An edit made by hand across many files can leave one of them unparsable (a
// lost indent, an unbalanced brace) with nothing to show it until the tests
// run. One check call parses every changed file and names the lines, whatever
// the language: the backend's grammars for all of them, and Python's own
// parser for Python, whose indentation errors a tree-sitter grammar accepts.

const CHECK_MAX_FILES = 2000;
// One request to the daemon holds at most 1 MiB of JSON; a batch stays well
// below it, and a file that would not fit alone is not checked.
const CHECK_MAX_FILE_BYTES = 512 * 1024;
const CHECK_BATCH_BYTES = 640 * 1024;
const CHECK_BATCH_FILES = 200;
const CHECK_FILES_SHOWN = 20;
const CHECK_LINES_PER_FILE = 5;
const CHECK_LINE_MAX_CHARS = 160;
const CHECK_NAMES_SHOWN = 8;
const BASELINE_MAX_FILES = 40;
const PYTHON_TIMEOUT_MS = 20_000;
const PYTHON_FILE = /\.pyi?$/i;

export interface CheckRequest {
  /** Files or directories as given to `gcal grep`; empty means the files that differ from HEAD. */
  paths: string[];
}

export interface SourceText {
  path: string;
  content: string;
}

export interface PythonError {
  line: number;
  message: string;
}

/**
 * Parses Python sources with a Python interpreter: the first syntax error of
 * each file that has one. Undefined when no interpreter can be run.
 */
export type PythonChecker = (sources: SourceText[]) => Promise<Map<string, PythonError> | undefined>;

export interface CheckOptions {
  /** Replaces the interpreter lookup; `null` turns the Python check off. */
  python?: PythonChecker | null;
}

interface Diagnostic {
  line: number;
  note: string;
}

interface CheckedFile {
  path: string;
  status: "clean" | "errors" | "unsupported" | "unreadable";
  total: number;
  diagnostics: Diagnostic[];
}

export async function checkSyntax(
  context: CodeContext,
  request: CheckRequest,
  options: CheckOptions = {},
): Promise<ReadResult> {
  if (context.newFiles === undefined) {
    return { ok: false, text: "# gcal check needs a git working tree" };
  }
  const python = options.python === undefined ? pythonSyntaxErrors : options.python;

  const changedOnly = request.paths.length === 0;
  const files = (await filesToCheck(context, request.paths)).filter((path) => !BINARY_FILE.test(path));
  const subject = changedOnly ? "changed files" : "files";
  if (files.length === 0) {
    return {
      ok: true,
      text: changedOnly ? "# syntax check: no file differs from HEAD" : "# syntax check: no file under these paths",
    };
  }
  if (files.length > CHECK_MAX_FILES) {
    return {
      ok: false,
      text: `# syntax check: ${files.length} files; give narrower paths (at most ${CHECK_MAX_FILES})`,
    };
  }

  const sources: SourceText[] = [];
  const unreadable: string[] = [];
  for (const path of files) {
    const content = await readText(join(context.root, path));
    if (content === undefined) unreadable.push(path);
    else sources.push({ path, content });
  }

  let checked: CheckedFile[];
  try {
    checked = await parse(context, sources, python);
  } catch (error) {
    if (error instanceof Error && /unknown tool/i.test(error.message)) {
      return { ok: false, text: "# gcal check needs a Goldeneye build that has the check_syntax tool" };
    }
    throw error;
  }

  const withErrors = checked.filter((file) => file.status === "errors");
  const baseline = await baselineTotals(context, withErrors, python);
  const broken = withErrors.filter((file) => file.total > (baseline.get(file.path) ?? 0));
  const unchanged = withErrors.filter((file) => !broken.includes(file));
  const unsupported = checked.filter((file) => file.status === "unsupported");
  const parsed = checked.length - unsupported.length;

  const output: string[] = [];
  if (broken.length === 0) {
    output.push(`# syntax check: ${parsed} ${subject} parse`);
  } else {
    output.push(`# syntax check: ${broken.length} of ${parsed} ${subject} do not parse`);
    const contents = new Map(sources.map((source) => [source.path, source.content]));
    for (const file of broken.slice(0, CHECK_FILES_SHOWN)) {
      output.push(file.path, ...diagnosticRows(file, contents.get(file.path) ?? ""));
    }
    if (broken.length > CHECK_FILES_SHOWN) {
      output.push(
        `# ... ${broken.length - CHECK_FILES_SHOWN} more files: ${names(broken.slice(CHECK_FILES_SHOWN).map((file) => file.path))}`,
      );
    }
  }
  if (unchanged.length > 0) {
    output.push(
      `# ${unchanged.length} with the same diagnostics as at HEAD, not caused by the change: ${names(unchanged.map((file) => file.path))}`,
    );
  }
  if (unreadable.length > 0) output.push(`# ${unreadable.length} could not be read: ${names(unreadable)}`);
  if (unsupported.length > 0) {
    output.push(
      `# ${unsupported.length} not checked (no grammar): ${names(unsupported.map((file) => file.path))}`,
    );
  }
  return { ok: true, text: output.join("\n") };
}

async function filesToCheck(context: CodeContext, paths: string[]): Promise<string[]> {
  const newFiles = context.newFiles!;
  if (paths.length === 0) return newFiles.changed();
  const given = separatePaths(context.root, paths);
  const scoped = given.filter((path) => !/^\.?[\\/]*$/.test(path));
  const all = await newFiles.all();
  if (scoped.length < given.length) return all;
  const inScope = new RegExp(pathsPattern(context.root, scoped));
  return all.filter((path) => inScope.test(path));
}

/** The text of a file small enough to check; undefined when it is missing, large or binary. */
async function readText(absolute: string): Promise<string | undefined> {
  try {
    if ((await stat(absolute)).size > CHECK_MAX_FILE_BYTES) return undefined;
    const text = await readFile(absolute, "utf8");
    return text.includes("\u0000") ? undefined : text;
  } catch {
    return undefined;
  }
}

/**
 * The text is sent rather than a path: the backend then needs no access to the
 * working tree, which a daemon started elsewhere does not have.
 */
async function parse(
  context: CodeContext,
  sources: SourceText[],
  python: PythonChecker | null,
): Promise<CheckedFile[]> {
  const checked = new Map<string, CheckedFile>();
  let batch: SourceText[] = [];
  let batchBytes = 0;
  const flush = async () => {
    if (batch.length === 0) return;
    const raw = await context.client.tool("check_syntax", { contents: batch });
    for (const file of parseFiles(raw)) checked.set(file.path, file);
    batch = [];
    batchBytes = 0;
  };
  for (const source of sources) {
    // The size on the wire: JSON escapes quotes, backslashes and line ends.
    const bytes = Buffer.byteLength(JSON.stringify(source));
    if (bytes > CHECK_BATCH_BYTES) continue; // reported as not checked
    if (batch.length >= CHECK_BATCH_FILES || batchBytes + bytes > CHECK_BATCH_BYTES) {
      await flush();
    }
    batch.push(source);
    batchBytes += bytes;
  }
  await flush();

  const pythonSources = sources.filter((source) => PYTHON_FILE.test(source.path));
  const pythonErrors =
    python === null || pythonSources.length === 0 ? undefined : await python(pythonSources);
  if (pythonErrors !== undefined) {
    // The interpreter is the authority on Python: it replaces the grammar's verdict.
    for (const source of pythonSources) {
      const error = pythonErrors.get(source.path);
      checked.set(source.path, {
        path: source.path,
        status: error === undefined ? "clean" : "errors",
        total: error === undefined ? 0 : 1,
        diagnostics: error === undefined ? [] : [{ line: error.line, note: error.message }],
      });
    }
  }

  return sources.map(
    (source) =>
      checked.get(source.path) ?? { path: source.path, status: "unsupported", total: 0, diagnostics: [] },
  );
}

/**
 * Diagnostics the committed version of each file already has. A grammar or an
 * interpreter that lags behind the language reports errors in valid code;
 * those must not be read as damage done by the edit.
 */
async function baselineTotals(
  context: CodeContext,
  files: CheckedFile[],
  python: PythonChecker | null,
): Promise<Map<string, number>> {
  const committed: SourceText[] = [];
  for (const file of files.slice(0, BASELINE_MAX_FILES)) {
    const content = await context.newFiles!.atHead(file.path);
    if (content !== undefined) committed.push({ path: file.path, content });
  }
  if (committed.length === 0) return new Map();
  try {
    const checked = await parse(context, committed, python);
    return new Map(checked.map((file) => [file.path, file.total]));
  } catch {
    // Without a baseline every diagnostic counts as new.
    return new Map();
  }
}

function diagnosticRows(file: CheckedFile, content: string): string[] {
  const lines = content.split("\n");
  const seen = new Set<number>();
  const rows: string[] = [];
  for (const diagnostic of file.diagnostics) {
    if (seen.has(diagnostic.line)) continue;
    seen.add(diagnostic.line);
    if (rows.length >= CHECK_LINES_PER_FILE) continue;
    const text = (lines[diagnostic.line - 1] ?? "").trim().replace(/\s+/g, " ");
    const shown = text.length > CHECK_LINE_MAX_CHARS ? `${text.slice(0, CHECK_LINE_MAX_CHARS - 1)}…` : text;
    rows.push(`  ${diagnostic.line}: ${shown}${diagnostic.note === "" ? "" : `\t${diagnostic.note}`}`);
  }
  const hidden = Math.max(file.total, seen.size) - rows.length;
  if (hidden > 0) rows.push(`  # ... ${hidden} more`);
  return rows;
}

function names(paths: string[]): string {
  const shown = paths.slice(0, CHECK_NAMES_SHOWN);
  return paths.length > shown.length ? `${shown.join(", ")}, +${paths.length - shown.length}` : shown.join(", ");
}

function parseFiles(raw: unknown): CheckedFile[] {
  if (!isRecord(raw) || !Array.isArray(raw.files)) return [];
  return raw.files.filter(isRecord).map((file) => {
    const status = file.status === "clean" || file.status === "errors" ? file.status : "unsupported";
    return {
      path: typeof file.path === "string" ? file.path : "",
      status,
      total: typeof file.diagnostic_total === "number" ? file.diagnostic_total : 0,
      diagnostics: (Array.isArray(file.diagnostics) ? file.diagnostics : []).filter(isRecord).map((item) => ({
        line: startRow(item.span) + 1,
        note: item.kind === "missing" && typeof item.node_kind === "string" ? `missing ${item.node_kind}` : "",
      })),
    };
  });
}

/** Zero-based first row of a backend source span. */
function startRow(span: unknown): number {
  if (isRecord(span) && isRecord(span.start) && typeof span.start.row === "number") return span.start.row;
  return 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const PYTHON_COMMANDS: Array<[string, string[]]> = [
  ["python", []],
  ["python3", []],
  ["py", ["-3"]],
];

// Parses only: nothing of the checked sources is imported or run.
const PYTHON_SCRIPT = [
  "import ast, json, sys",
  "found = {}",
  "for item in json.load(sys.stdin):",
  "    try:",
  "        ast.parse(item['content'], filename=item['path'])",
  "    except SyntaxError as error:",
  "        found[item['path']] = [error.lineno or 1, error.msg or 'invalid syntax']",
  "    except Exception as error:",
  "        found[item['path']] = [1, str(error)]",
  "json.dump(found, sys.stdout)",
].join("\n");

/** The default checker: the first Python interpreter on PATH that answers. */
export const pythonSyntaxErrors: PythonChecker = async (sources) => {
  for (const [command, prefix] of PYTHON_COMMANDS) {
    const output = await runPython(command, [...prefix, "-I", "-X", "utf8", "-c", PYTHON_SCRIPT], sources);
    if (output === undefined) continue;
    try {
      const found = JSON.parse(output) as Record<string, [number, string]>;
      return new Map(
        Object.entries(found).map(([path, [line, message]]) => [path, { line, message: String(message) }]),
      );
    } catch {
      continue;
    }
  }
  return undefined;
};

function runPython(command: string, args: string[], sources: SourceText[]): Promise<string | undefined> {
  return new Promise((resolve) => {
    let settled = false;
    const finish = (value: string | undefined) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve(value);
    };
    const child = spawn(command, args, { stdio: ["pipe", "pipe", "ignore"], windowsHide: true });
    const timer = setTimeout(() => {
      child.kill();
      finish(undefined);
    }, PYTHON_TIMEOUT_MS);
    let output = "";
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk: string) => {
      output += chunk;
    });
    child.on("error", () => finish(undefined));
    child.on("close", (code) => finish(code === 0 ? output : undefined));
    child.stdin.on("error", () => finish(undefined));
    child.stdin.end(JSON.stringify(sources));
  });
}
