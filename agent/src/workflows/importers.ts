import { readFile } from "node:fs/promises";
import { join, posix } from "node:path";
import { allFilePaths } from "../kernel/codeGraph.js";
import { isTestPath } from "../kernel/displayName.js";
import type { CodeContext, ReadResult } from "./readCode.js";

// Moving or renaming a module means finding every file that names it: imports,
// re-exports, dynamic imports and test mocks, under every spelling of its path
// (alias, relative, with or without extension). `importers` lists them in one
// answer, with the lines that use what each file imports.

const SCRIPT = ["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "vue", "svelte", "astro", "mdx"];
const STYLE = ["css", "scss", "sass", "less"];
const C_FAMILY = ["c", "h", "cc", "cpp", "cxx", "hh", "hpp", "hxx", "m", "mm"];
const PYTHON = ["py", "pyi"];
/** Languages that import a type or a package, not a file: the caller answers with the type. */
const BY_NAME = new Set([
  "java", "kt", "kts", "scala", "groovy", "cs", "go", "rs", "swift", "php", "rb", "dart", "ex", "exs", "hs", "fs", "vb",
]);

const SCAN_MAX_FILES = 20_000;
const SCAN_BATCH = 32;
const SCAN_MAX_CHARS = 2 * 1024 * 1024;
const MIN_FILES_SHOWN = 80;
const USE_LINES_SHOWN = 8;
const NAMES_SHOWN = 6;
const HEADER_NAMES_SHOWN = 12;
const STATEMENT_MAX_LINES = 40;
const LINE_MAX_CHARS = 160;

interface Target {
  path: string;
  kind: "script" | "python" | "c" | "asset";
  /** Paths a specifier may resolve to. */
  forms: Set<string>;
  /** Dotted module name, for Python. */
  module: string;
  depth: number;
  directory: string;
}

interface Importer {
  filePath: string;
  /** Lines that name the module, 1-based. */
  lines: number[];
  /** Lines taken by the statements that name it. */
  statementLines: Set<number>;
  specifiers: string[];
  /** Local names bound from the module. */
  locals: Set<string>;
  /** Names as the module exports them. */
  exported: Set<string>;
  text: string[];
}

function extensionOf(path: string): string {
  return /\.([A-Za-z0-9]+)$/.exec(path)?.[1]?.toLowerCase() ?? "";
}

function withoutScriptExtension(path: string): string {
  return path.replace(/\.(d\.ts|tsx?|jsx?|mjs|cjs|mts|cts)$/, "");
}

function describeTarget(path: string): Target | undefined {
  const extension = extensionOf(path);
  if (BY_NAME.has(extension)) return undefined;
  const directory = posix.dirname(path);
  const base = { path, directory, depth: path.split("/").length, module: "" };
  if (SCRIPT.includes(extension)) {
    const bare = withoutScriptExtension(path);
    const forms = new Set([bare]);
    // `dir/index.ts` is imported as `dir`.
    if (posix.basename(bare) === "index" && directory !== ".") forms.add(directory);
    return { ...base, kind: "script", forms };
  }
  if (PYTHON.includes(extension)) {
    const module = path
      .replace(/\.pyi?$/, "")
      .replace(/\/__init__$/, "")
      .replace(/\//g, ".");
    return { ...base, kind: "python", forms: new Set([path]), module };
  }
  return { ...base, kind: C_FAMILY.includes(extension) ? "c" : "asset", forms: new Set([path]) };
}

function scannedExtensions(target: Target): string[] {
  if (target.kind === "script") return SCRIPT;
  if (target.kind === "python") return PYTHON;
  if (target.kind === "c") return C_FAMILY;
  return [...SCRIPT, ...STYLE, "html", "htm"];
}

/** Whether a path written in `importer` names the target. */
function refers(specifier: string, importer: string, target: Target): boolean {
  // Bundler suffixes: `./icon.svg?raw`.
  let clean = specifier.replace(/[?#].*$/, "");
  if (clean.length === 0) return false;
  if (target.kind === "script") clean = withoutScriptExtension(clean);
  const matches = (path: string) => target.forms.has(path);

  if (clean.startsWith(".")) {
    return matches(posix.normalize(posix.join(posix.dirname(importer), clean)));
  }
  if (target.kind === "c") {
    // Include directories are not known: the written path is a suffix of the real one.
    return [...target.forms].some((form) => form === clean || form.endsWith(`/${clean}`));
  }
  // An alias (`@/lib/x`, `~/lib/x`, `src/lib/x`) or a path from a source root.
  const segments = clean.replace(/^\//, "").split("/");
  for (const dropped of [0, 1]) {
    const tail = segments.slice(dropped);
    // One remaining segment says too little, unless the target sits near the root.
    if (tail.length === 0 || (tail.length === 1 && (dropped === 0 || target.depth > 2))) continue;
    const suffix = tail.join("/");
    if ([...target.forms].some((form) => form === suffix || form.endsWith(`/${suffix}`))) return true;
  }
  return false;
}

const QUOTED = /(["'`])([^"'`\s]+)\1/g;
const INCLUDE = /^\s*#\s*(?:include|import)\s*[<"]([^>"]+)[>"]/;

function scriptNames(statement: string, importer: Importer): void {
  // A mock or a path in other code binds nothing.
  if (!/^\s*(import|export)\b/.test(statement) && !/=\s*(await\s+)?(require|import)\s*\(/.test(statement)) return;
  const named = /\{([^}]*)\}/.exec(statement)?.[1];
  const add = (exported: string, local: string) => {
    if (!/^[A-Za-z_$][\w$]*$/.test(local)) return;
    importer.locals.add(local);
    if (/^[A-Za-z_$][\w$]*$/.test(exported)) importer.exported.add(exported);
  };
  if (named !== undefined) {
    for (const part of named.split(",")) {
      // `a`, `a as b`, `type A`, and the destructuring form `a: b`.
      const [exported, local] = part
        .trim()
        .replace(/^type\s+/, "")
        .split(/\s+as\s+|\s*:\s*/);
      if (exported) add(exported, local ?? exported);
    }
  }
  const namespace = /\*\s+as\s+([A-Za-z_$][\w$]*)/.exec(statement)?.[1];
  if (namespace !== undefined) importer.locals.add(namespace);
  const byDefault = /\bimport\s+(?:type\s+)?([A-Za-z_$][\w$]*)\s*(?:,|from\b)/.exec(statement)?.[1];
  if (byDefault !== undefined && byDefault !== "type") importer.locals.add(byDefault);
  const required = /\b(?:const|let|var)\s+([A-Za-z_$][\w$]*)\s*=\s*(?:await\s+)?(?:require|import)\s*\(/.exec(statement)?.[1];
  if (required !== undefined) importer.locals.add(required);
}

/** The lines of the import or export statement that ends on `index` (0-based). */
function statementStart(lines: string[], index: number): number {
  if (/^\s*(import|export)\b/.test(lines[index]) || /\b(require|import)\s*\(/.test(lines[index])) return index;
  for (let back = index - 1; back >= Math.max(0, index - STATEMENT_MAX_LINES); back -= 1) {
    if (/^\s*(import|export)\b/.test(lines[back])) return back;
    if (/;\s*$/.test(lines[back])) break;
  }
  return index;
}

function scanPaths(importer: Importer, target: Target): void {
  const lines = importer.text;
  for (let index = 0; index < lines.length; index += 1) {
    const line = lines[index];
    const specifiers: string[] = [];
    const include = target.kind === "c" ? INCLUDE.exec(line)?.[1] : undefined;
    if (include !== undefined) specifiers.push(include);
    if (target.kind !== "c") {
      for (const match of line.matchAll(QUOTED)) specifiers.push(match[2]);
    }
    const found = specifiers.filter((specifier) => refers(specifier, importer.filePath, target));
    if (found.length === 0) continue;

    importer.lines.push(index + 1);
    importer.specifiers.push(...found);
    const start = target.kind === "script" ? statementStart(lines, index) : index;
    for (let covered = start; covered <= index; covered += 1) importer.statementLines.add(covered + 1);
    if (target.kind === "script") scriptNames(lines.slice(start, index + 1).join(" "), importer);
  }
}

function pythonModuleMatches(written: string, importer: string, target: Target): boolean {
  if (written === target.module) return true;
  if (!target.module.endsWith(`.${written}`)) return false;
  // A single name is only trusted next to the importer or near the root.
  return written.includes(".") || posix.dirname(importer) === target.directory || target.depth <= 2;
}

function scanPython(importer: Importer, target: Target): void {
  const lines = importer.text;
  const hit = (start: number, end: number, specifier: string) => {
    importer.lines.push(start + 1);
    importer.specifiers.push(specifier);
    for (let covered = start; covered <= end; covered += 1) importer.statementLines.add(covered + 1);
  };
  const bind = (exported: string, local: string) => {
    importer.locals.add(local);
    importer.exported.add(exported);
  };

  for (let index = 0; index < lines.length; index += 1) {
    const from = /^\s*from\s+(\.*)([\w.]*)\s+import\s+(.*)$/.exec(lines[index]);
    if (from !== null) {
      let end = index;
      let list = from[3];
      if (list.includes("(") && !list.includes(")")) {
        while (end + 1 < lines.length && end - index < STATEMENT_MAX_LINES && !lines[end].includes(")")) {
          end += 1;
          list += ` ${lines[end]}`;
        }
      }
      const names = list
        .replace(/#.*$/, "")
        .replace(/[()\\]/g, " ")
        .split(",")
        .map((part) => part.trim().split(/\s+as\s+/))
        .filter(([name]) => /^[\w*]+$/.test(name ?? ""));

      let written = from[2];
      if (from[1].length > 0) {
        // `.` is the importer's package, each further dot one package up.
        const packageParts = posix.dirname(importer.filePath).split("/").filter((part) => part !== ".");
        const base = packageParts.slice(0, packageParts.length - (from[1].length - 1));
        written = [...base, ...(written.length > 0 ? [written] : [])].join(".");
      }
      const specifier = `${from[1]}${from[2]}`;
      if (written.length > 0 && pythonModuleMatches(written, importer.filePath, target)) {
        hit(index, end, specifier);
        for (const [name, alias] of names) if (name !== "*") bind(name, alias ?? name);
      } else {
        // `from package import module`.
        for (const [name, alias] of names) {
          const full = written.length > 0 ? `${written}.${name}` : name;
          if (full.includes(".") && pythonModuleMatches(full, importer.filePath, target)) {
            hit(index, end, `${specifier} import ${name}`);
            importer.locals.add(alias ?? name);
          }
        }
      }
      index = end;
      continue;
    }

    const plain = /^\s*import\s+(.+)$/.exec(lines[index]);
    if (plain === null) continue;
    for (const part of plain[1].replace(/#.*$/, "").split(",")) {
      const [module, alias] = part.trim().split(/\s+as\s+/);
      if (!/^[\w.]+$/.test(module ?? "") || !pythonModuleMatches(module, importer.filePath, target)) continue;
      hit(index, index, module);
      importer.locals.add(alias ?? module.split(".").at(-1)!);
    }
  }
}

function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function lineList(lines: number[]): string {
  const shown = lines.slice(0, USE_LINES_SHOWN).join(",");
  return lines.length > USE_LINES_SHOWN ? `${shown},+${lines.length - USE_LINES_SHOWN}` : shown;
}

function usesRow(importer: Importer, names: string[], bound: boolean): string | undefined {
  const fields: string[] = [];
  for (const name of bound ? names.slice(0, NAMES_SHOWN) : names) {
    const word = new RegExp(`(?<![\\w$])${escapeRegExp(name)}(?![\\w$])`);
    const used = importer.text.flatMap((line, index) =>
      !importer.statementLines.has(index + 1) && word.test(line) ? [index + 1] : [],
    );
    // An imported name that is not used is worth a row; a name the file never bound is not.
    if (used.length > 0) fields.push(`${name} ${lineList(used)}`);
    else if (bound) fields.push(`${name} -`);
  }
  if (fields.length === 0) return undefined;
  const more = bound && names.length > NAMES_SHOWN ? `; +${names.length - NAMES_SHOWN} more names` : "";
  return `  uses\t${fields.join("; ")}${more}`;
}

/**
 * The files that name the module at `path`, or undefined when its language
 * imports by type or package and not by file.
 */
export async function importers(
  context: CodeContext,
  path: string,
  options: { limit: number; exists: boolean },
): Promise<ReadResult | undefined> {
  const target = describeTarget(path);
  if (target === undefined) return undefined;

  const extensions = new Set(scannedExtensions(target));
  const tree = (await context.newFiles?.all()) ?? [];
  const everything = tree.length > 0 ? tree : await allFilePaths(context.client);
  const files = everything.filter((filePath) => filePath !== path && extensions.has(extensionOf(filePath)));
  if (files.length > SCAN_MAX_FILES) {
    return { ok: false, text: `# too many files to scan for importers of ${path}: ${files.length}; use gcal grep` };
  }

  // Every importer names the file by its base name (or its directory, for an index file).
  const stem = posix.basename(target.kind === "script" ? withoutScriptExtension(path) : path).replace(/\.pyi?$/, "");
  const hint = stem === "index" || stem === "__init__" ? posix.basename(target.directory) : stem;

  const found: Importer[] = [];
  for (let start = 0; start < files.length; start += SCAN_BATCH) {
    const scanned = await Promise.all(
      files.slice(start, start + SCAN_BATCH).map(async (filePath): Promise<Importer | undefined> => {
        let text: string;
        try {
          text = await readFile(join(context.root, filePath), "utf8");
        } catch {
          return undefined;
        }
        if (text.length > SCAN_MAX_CHARS || !text.includes(hint)) return undefined;
        const importer: Importer = {
          filePath,
          lines: [],
          statementLines: new Set(),
          specifiers: [],
          locals: new Set(),
          exported: new Set(),
          text: text.split(/\r?\n/),
        };
        if (target.kind === "python") scanPython(importer, target);
        else scanPaths(importer, target);
        return importer.lines.length > 0 ? importer : undefined;
      }),
    );
    found.push(...scanned.filter((importer): importer is Importer => importer !== undefined));
  }

  const gone = options.exists ? "" : " (no such file now)";
  if (found.length === 0) {
    return { ok: true, text: `# importers of ${path}${gone}: 0 (scanned ${files.length} files)` };
  }

  found.sort(
    (left, right) =>
      Number(isTestPath(left.filePath)) - Number(isTestPath(right.filePath)) ||
      left.filePath.localeCompare(right.filePath),
  );
  const count = (values: string[]) => {
    const counts = new Map<string, number>();
    for (const value of values) counts.set(value, (counts.get(value) ?? 0) + 1);
    return [...counts].sort((left, right) => right[1] - left[1]);
  };
  const specifiers = count(found.flatMap((importer) => importer.specifiers));
  // What files import, counted by file.
  const exported = count(found.flatMap((importer) => [...importer.exported]));
  const everyExported = exported.map(([name]) => name);
  const tests = found.filter((importer) => isTestPath(importer.filePath)).length;

  const lines = [
    `# importers of ${path}${gone}: ${found.length} files${tests > 0 ? ` (${tests} tests)` : ""}; written as ${specifiers
      .map(([specifier, files]) => `"${specifier}" ${files}`)
      .join(", ")}`,
  ];
  if (exported.length > 0) {
    const more = exported.length > HEADER_NAMES_SHOWN ? `, +${exported.length - HEADER_NAMES_SHOWN} more` : "";
    lines.push(
      `# imported names (files): ${exported
        .slice(0, HEADER_NAMES_SHOWN)
        .map(([name, files]) => `${name} ${files}`)
        .join(", ")}${more}`,
    );
  }

  const shown = found.slice(0, Math.max(options.limit, MIN_FILES_SHOWN));
  for (const importer of shown) {
    lines.push(importer.filePath);
    for (const line of importer.lines) {
      const text = (importer.text[line - 1] ?? "").replace(/\s+$/, "");
      lines.push(`  ${line}\t${text.length > LINE_MAX_CHARS ? `${text.slice(0, LINE_MAX_CHARS - 1)}…` : text}`);
    }
    // A file that names the module without binding anything (a mock, a dynamic
    // import) is checked for the names the other files import.
    const bound = importer.locals.size > 0;
    const uses = usesRow(importer, bound ? [...importer.locals] : everyExported, bound);
    if (uses !== undefined) lines.push(uses);
  }
  if (found.length > shown.length) {
    lines.push(`# ... ${found.length - shown.length} more files; rerun with --limit ${found.length}`);
  }
  return { ok: true, text: lines.join("\n") };
}
