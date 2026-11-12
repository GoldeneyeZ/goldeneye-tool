import { lstat, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { BINARY_FILE, pathsPattern, separatePaths, undoubled, undoubledNote } from "./exploreCode.js";
import type { CodeContext, ReadResult } from "./readCode.js";

// A rename across many files costs one model turn per patch when it is written
// by hand. One replace call does the mechanical part and prints every changed
// line, so the result is reviewed in the same turn.

export const REPLACE_MAX_LINES_SHOWN = 80;
const REPLACE_MAX_FILE_BYTES = 2 * 1024 * 1024;
const REPLACE_LINE_MAX_CHARS = 200;
const OUTSIDE_FILES_SHOWN = 40;

export interface ReplaceRequest {
  pattern: string;
  replacement: string;
  /** Files or directories, as given to `gcal grep`; `.` is the whole repository. */
  paths: string[];
  /** The pattern is literal text, and so is the replacement. */
  literal: boolean;
  /** Report what would change and write nothing. */
  dryRun: boolean;
}

interface FileChange {
  filePath: string;
  count: number;
  /** Changed lines as they are after the replacement; empty when lines were added or removed. */
  lines: Array<{ line: number; text: string }>;
}

export async function replaceText(context: CodeContext, request: ReplaceRequest): Promise<ReadResult> {
  if (context.newFiles === undefined) {
    return { ok: false, text: "# gcal replace needs a git working tree" };
  }
  if (request.paths.length === 0) {
    return {
      ok: false,
      text: "# gcal replace needs where to replace: files, directories, or . for the whole repository",
    };
  }
  if (request.pattern.length === 0) return { ok: false, text: "# gcal replace needs a pattern" };

  const expression = compile(request.pattern, request.literal);
  if (expression === undefined) {
    return {
      ok: false,
      text: `# not a regular expression: ${request.pattern} (literal text: gcal replace -F)`,
    };
  }

  const given = separatePaths(context.root, request.paths);
  const scoped = given.filter((path) => !/^\.?[\\/]*$/.test(path));
  const inScope =
    scoped.length === given.length
      ? new RegExp(pathsPattern(context.root, scoped))
      : undefined;
  const everything = await context.newFiles.all();
  const files = everything.filter(
    (path) => !BINARY_FILE.test(path) && (inScope === undefined || inScope.test(path)),
  );

  const replaceAll = async (pattern: RegExp): Promise<FileChange[]> => {
    const changes: FileChange[] = [];
    for (const filePath of files) {
      const change = await replaceInFile(context.root, filePath, pattern, request);
      if (change !== undefined) changes.push(change);
    }
    return changes;
  };
  // A rename given a list of files is complete only if the list was: say where
  // else the pattern still matches.
  const elsewhere = async (pattern: RegExp): Promise<string[]> => {
    if (inScope === undefined) return [];
    const outside = everything.filter((path) => !BINARY_FILE.test(path) && !inScope.test(path));
    const found: string[] = [];
    for (const filePath of outside) {
      const count = await countMatches(context.root, filePath, pattern);
      if (count > 0) found.push(`${filePath} ${count}`);
    }
    if (found.length === 0) return [];
    const shown = found.slice(0, OUTSIDE_FILES_SHOWN).join(", ");
    const more = found.length > OUTSIDE_FILES_SHOWN ? `, +${found.length - OUTSIDE_FILES_SHOWN} more files` : "";
    return [`# the pattern also matches outside these paths, unchanged (path occurrences): ${shown}${more}`];
  };

  const changes = await replaceAll(expression);
  if (changes.length > 0) {
    return { ok: true, text: [render(changes, request.dryRun), ...(await elsewhere(expression))].join("\n") };
  }

  // `\\b` typed inside double quotes arrives as two backslashes and matches
  // nothing; the pattern meant is the one with single backslashes.
  const single = request.literal ? undefined : undoubled(request.pattern);
  const retried = single === undefined ? undefined : compile(single, false);
  if (single !== undefined && retried !== undefined) {
    const again = await replaceAll(retried);
    if (again.length > 0) {
      return {
        ok: true,
        text: [
          undoubledNote(request.pattern, single),
          render(again, request.dryRun),
          ...(await elsewhere(retried)),
        ].join("\n"),
      };
    }
  }
  return { ok: false, text: `# no match: ${request.pattern} in ${request.paths.join(" ")}` };
}

function compile(pattern: string, literal: boolean): RegExp | undefined {
  if (literal) return new RegExp(pattern.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), "g");
  // rg spells case-insensitivity inside the pattern.
  const insensitive = pattern.startsWith("(?i)");
  try {
    return new RegExp(insensitive ? pattern.slice(4) : pattern, insensitive ? "gim" : "gm");
  } catch {
    return undefined;
  }
}

/** The text of a plain, text file small enough to rewrite; undefined for anything else. */
async function readPlainText(absolute: string): Promise<string | undefined> {
  try {
    // A link may point outside the repository; only plain files are rewritten.
    const stats = await lstat(absolute);
    if (!stats.isFile() || stats.size > REPLACE_MAX_FILE_BYTES) return undefined;
    const text = await readFile(absolute, "utf8");
    return text.includes("\u0000") ? undefined : text;
  } catch {
    return undefined;
  }
}

async function countMatches(root: string, filePath: string, expression: RegExp): Promise<number> {
  const text = await readPlainText(join(root, filePath));
  return text === undefined ? 0 : [...text.matchAll(expression)].length;
}

async function replaceInFile(
  root: string,
  filePath: string,
  expression: RegExp,
  request: ReplaceRequest,
): Promise<FileChange | undefined> {
  const absolute = join(root, filePath);
  const before = await readPlainText(absolute);
  if (before === undefined) return undefined;

  const count = [...before.matchAll(expression)].length;
  if (count === 0) return undefined;
  const after = request.literal
    ? before.replace(expression, () => request.replacement)
    : before.replace(expression, request.replacement);
  if (after === before) return undefined;
  if (!request.dryRun) await writeFile(absolute, after);

  const oldLines = before.split("\n");
  const newLines = after.split("\n");
  const lines =
    oldLines.length === newLines.length
      ? newLines.flatMap((text, index) =>
          text === oldLines[index] ? [] : [{ line: index + 1, text: text.replace(/\r$/, "") }],
        )
      : [];
  return { filePath, count, lines };
}

function render(changes: FileChange[], dryRun: boolean): string {
  const occurrences = changes.reduce((sum, change) => sum + change.count, 0);
  const output = [
    `# ${dryRun ? "would replace" : "replaced"} ${occurrences} occurrence${occurrences === 1 ? "" : "s"} in ${changes.length} file${changes.length === 1 ? "" : "s"}`,
  ];
  let shown = 0;
  let hidden = 0;
  for (const change of changes) {
    output.push(`${change.filePath}\t${change.count}`);
    for (const { line, text } of change.lines) {
      if (shown >= REPLACE_MAX_LINES_SHOWN) {
        hidden += 1;
        continue;
      }
      shown += 1;
      const trimmed = text.trim();
      output.push(
        `  ${line}: ${trimmed.length > REPLACE_LINE_MAX_CHARS ? `${trimmed.slice(0, REPLACE_LINE_MAX_CHARS)}…` : trimmed}`,
      );
    }
  }
  if (hidden > 0) output.push(`# ... ${hidden} more changed lines not shown: git diff`);
  return output.join("\n");
}
