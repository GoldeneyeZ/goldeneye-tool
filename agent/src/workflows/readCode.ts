import { basename, isAbsolute, relative } from "node:path";
import type { NewFiles } from "../kernel/newFiles.js";
import {
  filesBySuffix,
  isContainerNode,
  isTypeNode,
  membersOf,
  nodesAtLine,
  nodesInFile,
  textSearch,
  type GraphNode,
  type TextMatch,
  type ToolClient,
} from "../kernel/codeGraph.js";
import { displayName, isTestPath, location } from "../kernel/displayName.js";
import type { FreshnessProbe } from "../kernel/freshness.js";
import {
  couldBeFileName,
  normalizePath,
  parseLocator,
  parseSymbolLocator,
  type FileLocator,
  type SymbolLocator,
} from "../kernel/locator.js";
import { locateDeclaration } from "../kernel/relocate.js";
import type { SourceReader } from "../kernel/sourceFiles.js";
import { resolveSymbol, soleSymbol } from "./resolveSymbol.js";

// A whole mid-size type costs fewer tokens than the extra model turn an
// outline-then-read needs; only larger ones start as an outline.
export const FULL_SOURCE_MAX_LINES = 500;
// In a batch over the output budget, whole types above this fall back to outlines.
export const BATCH_OUTLINE_MIN_LINES = 150;
export const LARGE_CALLABLE_HEAD_LINES = 120;
export const RANGE_MAX_LINES = 400;
export const OUTLINE_MAX_ROWS = 200;
export const READ_MAX_OUTPUT_BYTES = 48 * 1024;
const LINE_WINDOW = 15;
const LEADING_DOC_MAX_LINES = 80;
const AMBIGUOUS_INLINE_MAX = 3;
const AMBIGUOUS_INLINE_MAX_LINES = 40;
const AMBIGUOUS_FILE = "# ambiguous file:";
const DIRECTORY_MAX_FILES = 120;
const ALSO_DECLARED_MAX = 3;
const DECLARATION_MAX_CHARS = 160;
const STALE_NOTE = "# note: file changed since indexing; located by text (refresh: gcal index .)";
const STALE_LOST_NOTE =
  "# warning: file changed since indexing and the declaration was not found; lines may be off";

export interface CodeContext {
  client: ToolClient;
  project: string;
  root: string;
  reader: SourceReader;
  freshness: FreshnessProbe;
  /** Files created after indexing; absent when the working tree cannot be inspected. */
  newFiles?: NewFiles;
}

export interface ReadResult {
  text: string;
  ok: boolean;
  /** A smaller rendering (an outline) for when a batch exceeds the output budget. */
  compact?: () => Promise<string>;
}

export interface ReadOptions {
  /**
   * auto: whole source up to FULL_SOURCE_MAX_LINES, an outline above;
   * full: whole source whatever the size; outline: always an outline.
   */
  detail?: "auto" | "full" | "outline";
}

export async function readLocators(
  context: CodeContext,
  raws: string[],
  options: ReadOptions = {},
): Promise<ReadResult> {
  const results: ReadResult[] = [];
  for (const raw of raws) {
    try {
      results.push(await readLocator(context, raw, options));
    } catch (error) {
      // One item the backend cannot answer must not cost the whole batch.
      if (raws.length === 1) throw error;
      const reason = error instanceof Error ? error.message.split("\n")[0] : String(error);
      results.push({
        ok: false,
        text: `# failed: ${raw}: ${reason}; read it by range: gcal get ${raw.replace(/:\d+(-\d+)?$/, "")}:1-${RANGE_MAX_LINES}`,
      });
    }
  }

  // Over budget: the largest whole types become outlines first, so every item
  // stays usable instead of being cut mid-body.
  const texts = results.map((result) => result.text);
  // An explicit --full for a single item is honored up to the byte budget.
  const mayCompact = (options.detail ?? "auto") === "auto" || results.length > 1;
  const compactable = new Set(
    results.flatMap((result, index) => (mayCompact && result.compact ? [index] : [])),
  );
  while (compactable.size > 0 && totalBytes(texts) > READ_MAX_OUTPUT_BYTES) {
    const largest = [...compactable].reduce((left, right) =>
      byteLength(texts[right]) > byteLength(texts[left]) ? right : left,
    );
    compactable.delete(largest);
    texts[largest] = await results[largest].compact!();
  }

  const budgets = fairBudgets(texts.map(byteLength), READ_MAX_OUTPUT_BYTES);
  return {
    text: texts.map((text, index) => truncateBytes(text, budgets[index])).join("\n\n"),
    // One miss in a batch is reported in the text; failing the command for it
    // would also cancel whatever the caller chained after it with `&&`.
    ok: results.some((result) => result.ok),
  };
}

export async function readLocator(
  context: CodeContext,
  raw: string,
  options: ReadOptions = {},
): Promise<ReadResult> {
  const locator = parseLocator(raw);
  if (locator.kind === "file") return readFileLocator(context, locator, options);

  const resolution = await resolveSymbol(context.client, context.project, locator);
  if (resolution.kind === "resolved") {
    const blocks: string[] = [];
    for (const node of resolution.nodes) blocks.push(await renderSymbol(context, node, options));
    const outlinable = (node: GraphNode) =>
      isTypeNode(node) && node.endLine - node.startLine + 1 > BATCH_OUTLINE_MIN_LINES;
    const declared = resolution.declarations ?? [];
    const alsoDeclared =
      declared.length === 0
        ? ""
        : `\n# also declared: ${declared.slice(0, ALSO_DECLARED_MAX).map(location).join(", ")}${
            declared.length > ALSO_DECLARED_MAX ? `, +${declared.length - ALSO_DECLARED_MAX}` : ""
          }`;
    return {
      text: blocks.join("\n\n") + alsoDeclared,
      ok: true,
      compact:
        options.detail !== "outline" && resolution.nodes.some(outlinable)
          ? async () => {
              const compacted = [];
              for (const [index, node] of resolution.nodes.entries()) {
                compacted.push(outlinable(node) ? await renderOutline(context, node) : blocks[index]);
              }
              return compacted.join("\n\n") + alsoDeclared;
            }
          : undefined,
    };
  }

  if (resolution.kind === "ambiguous") {
    return renderAmbiguous(context, locator, resolution.nodes);
  }

  // `Makefile` and `Dockerfile` look like symbols; the working tree knows better.
  const namesFile = async () =>
    ((await context.newFiles?.all()) ?? []).some(
      (path) => path === locator.raw || path.endsWith(`/${locator.raw}`),
    );
  if (couldBeFileName(locator) || (await namesFile())) {
    const asFile = await readFileLocator(
      context,
      {
        kind: "file",
        raw: locator.raw,
        path: normalizePath(locator.raw),
        startLine: undefined,
        endLine: undefined,
      },
      options,
    );
    if (asFile.ok || asFile.text.startsWith(AMBIGUOUS_FILE)) return asFile;
  }

  const asDirectory = await readDirectory(context, locator.raw);
  if (asDirectory !== undefined) return asDirectory;

  const inOwnerFile = await readMemberByText(context, locator);
  if (inOwnerFile !== undefined) return inOwnerFile;

  const unindexed = await readFromNewFiles(context, locator);
  if (unindexed !== undefined) return unindexed;

  // The name exists once, under another owner than the one given (a wrong
  // package, a method that lives on a helper class): that is the symbol meant.
  if (!resolution.similar) {
    const sole = soleSymbol(resolution.suggestions);
    if (sole !== undefined) {
      const blocks = [];
      for (const node of sole) blocks.push(await renderSymbol(context, node, options));
      return {
        ok: true,
        text: [
          `# not found: ${locator.raw}; showing the only symbol named ${sole[0].name}, ${displayName(sole[0], context.project)}`,
          blocks.join("\n\n"),
        ].join("\n"),
      };
    }
  }

  if (resolution.suggestions.length === 0) {
    const byText = await readByText(context, locator);
    if (byText !== undefined) return byText;
  }

  // A single close match is almost always what was meant (a plural, a typo):
  // answering with it saves the round trip a bare suggestion would cost.
  if (resolution.similar && resolution.suggestions.length === 1) {
    const [node] = resolution.suggestions;
    return {
      ok: true,
      text: [
        `# not found: ${locator.raw}; showing the only similar name, ${displayName(node, context.project)}`,
        await renderSymbol(context, node, options),
      ].join("\n"),
    };
  }

  return {
    ok: false,
    text: [
      `# not found: ${locator.raw}`,
      ...(resolution.suggestions.length > 0
        ? ["did you mean:", ...resolution.suggestions.map((node) => symbolRow(context, node))]
        : ["no symbol with that name is indexed; try: gcal grep <text>"]),
    ].join("\n"),
  };
}

const TEXT_LOCATED_NOTE = "# note: not in the index; declaration located by text";
const TEXT_FALLBACK_MAX_FILES = 6;
const TEXT_FALLBACK_MAX_ROWS = 6;
const TEXT_FALLBACK_LINE_MAX_CHARS = 160;

/**
 * `Owner.member` where the owner is indexed and the member is not: a member
 * added since indexing, or one the grammar does not extract. Its declaration
 * is looked for in the owner's file.
 */
async function readMemberByText(
  context: CodeContext,
  locator: SymbolLocator,
): Promise<ReadResult | undefined> {
  if (locator.segments.length < 2) return undefined;
  const member = locator.segments[locator.segments.length - 1];
  const owner = await resolveSymbol(
    context.client,
    context.project,
    parseSymbolLocator(locator.segments.slice(0, -1).join(".")),
  );
  if (owner.kind !== "resolved" || !isTypeNode(owner.nodes[0])) return undefined;

  const [type] = owner.nodes;
  const lines = await context.reader.lines(type.filePath);
  if (lines === undefined) return undefined;
  const range = locateDeclaration(lines, member, type.startLine);
  return range === undefined
    ? undefined
    : {
        ok: true,
        text: renderNewFile(locator.raw, type.filePath, lines, range, TEXT_LOCATED_NOTE),
      };
}

/**
 * A name the graph does not know at all (a macro, a symbol added since
 * indexing): its declaration when the text of a file shows one, otherwise the
 * lines that mention it, so the answer does not cost a second call.
 */
async function readByText(
  context: CodeContext,
  locator: SymbolLocator,
): Promise<ReadResult | undefined> {
  const name = locator.segments[locator.segments.length - 1];
  if (name === undefined || name.length < 3 || !/^\w+$/.test(name)) return undefined;

  let matches: TextMatch[];
  try {
    matches = (
      await textSearch(context.client, `\\b${name}\\b`, { regex: true, limit: 20 })
    ).matches;
  } catch {
    return undefined;
  }
  if (matches.length === 0) return undefined;

  const files = [...new Set(matches.map((match) => match.filePath))].sort(
    (left, right) => Number(isTestPath(left)) - Number(isTestPath(right)),
  );
  for (const filePath of files.slice(0, TEXT_FALLBACK_MAX_FILES)) {
    const lines = await context.reader.lines(filePath);
    const range = lines === undefined ? undefined : locateDeclaration(lines, name, 1);
    if (lines !== undefined && range !== undefined) {
      return {
        ok: true,
        text: renderNewFile(locator.raw, filePath, lines, range, TEXT_LOCATED_NOTE),
      };
    }
  }

  const rows: string[] = [];
  for (const match of matches) {
    const lines = await context.reader.lines(match.filePath);
    for (const line of match.lines) {
      if (rows.length >= TEXT_FALLBACK_MAX_ROWS) break;
      const text = (lines?.[line - 1] ?? "").trim().slice(0, TEXT_FALLBACK_LINE_MAX_CHARS);
      rows.push(`${match.filePath}:${line}: ${text}`);
    }
  }
  return {
    ok: true,
    text: [
      `# not found: ${locator.raw}; no symbol with that name is indexed. Text mentions (all: gcal grep ${name}):`,
      ...rows,
    ].join("\n"),
  };
}

const NEW_FILE_NOTE = "# note: new file, not indexed yet; read from the working tree";
const NEW_FILE_SCAN_MAX = 60;
const NEW_FILE_SCAN_MAX_LINES = 3000;

/**
 * A symbol the graph does not know may live in a file created after indexing:
 * first a new file named after one of the locator's segments, then any new
 * file that declares the last segment.
 */
async function readFromNewFiles(
  context: CodeContext,
  locator: SymbolLocator,
): Promise<ReadResult | undefined> {
  const files = (await context.newFiles?.list()) ?? [];
  if (files.length === 0 || locator.segments.length === 0) return undefined;

  const stem = (filePath: string) => basename(filePath).replace(/\.[^.]+$/, "");
  for (let index = locator.segments.length - 1; index >= 0; index -= 1) {
    const filePath = files.find((candidate) => stem(candidate) === locator.segments[index]);
    if (filePath === undefined) continue;
    const lines = await context.reader.lines(filePath);
    if (lines === undefined) continue;
    const member = locator.segments[index + 1];
    const range = member === undefined ? undefined : locateDeclaration(lines, member, 1);
    return { ok: true, text: renderNewFile(locator.raw, filePath, lines, range) };
  }

  const name = locator.segments[locator.segments.length - 1];
  for (const filePath of files.slice(0, NEW_FILE_SCAN_MAX)) {
    const lines = await context.reader.lines(filePath);
    if (lines === undefined || lines.length > NEW_FILE_SCAN_MAX_LINES) continue;
    const range = locateDeclaration(lines, name, 1);
    if (range !== undefined) {
      return { ok: true, text: renderNewFile(locator.raw, filePath, lines, range) };
    }
  }
  return undefined;
}

function renderNewFile(
  raw: string,
  filePath: string,
  lines: string[],
  range: { startLine: number; endLine: number } | undefined,
  note = NEW_FILE_NOTE,
): string {
  const start = range === undefined ? 1 : leadingDocStart(lines, range.startLine);
  const end = Math.min(
    range === undefined ? lines.length : range.endLine,
    start + FULL_SOURCE_MAX_LINES - 1,
  );
  return [
    `# ${raw} ${filePath}:${start}-${end}${end < lines.length && range === undefined ? ` of ${lines.length} lines` : ""}`,
    note,
    ...lines.slice(start - 1, end),
  ].join("\n");
}

export async function renderSymbol(
  context: CodeContext,
  node: GraphNode,
  options: ReadOptions = {},
): Promise<string> {
  const detail = options.detail ?? "auto";
  const lineCount = node.endLine - node.startLine + 1;
  if (
    isTypeNode(node) &&
    (detail === "outline" || (detail === "auto" && lineCount > FULL_SOURCE_MAX_LINES))
  ) {
    return renderOutline(context, node);
  }

  const fileLines = await context.reader.lines(node.filePath);
  if (fileLines === undefined) {
    const header = `# ${displayName(node, context.project)} ${node.label} ${location(node)}`;
    return `${header}\n${await backendSource(context, node)}`;
  }

  const current = await currentRange(context, fileLines, node);
  // The doc comment above a declaration is part of what a reader, and an edit, needs.
  const shown = { ...current.node, startLine: leadingDocStart(fileLines, current.node.startLine) };
  const header = `# ${displayName(node, context.project)} ${node.label} ${location(shown)}`;
  const lines = fileLines.slice(shown.startLine - 1, shown.endLine);
  if (lines.length <= FULL_SOURCE_MAX_LINES || detail === "full") {
    const detached = isTypeNode(node) ? await detachedMembers(context, node) : [];
    return [header, ...current.notes, ...lines, ...detached].join("\n");
  }

  const nextLine = shown.startLine + LARGE_CALLABLE_HEAD_LINES;
  return [
    header,
    ...current.notes,
    ...lines.slice(0, LARGE_CALLABLE_HEAD_LINES),
    `# ... ${lines.length - LARGE_CALLABLE_HEAD_LINES} more lines: gcal get ${node.filePath}:${nextLine}-${shown.endLine}`,
  ].join("\n");
}

const DOC_LINE = /^(\/\*|\*|\/\/|#\[|#!\[|#( |$)|@[A-Za-z])/;

/**
 * First line of the comment and annotation block directly above a declaration
 * (Javadoc, `///`, attributes, decorators), or the declaration's own line.
 */
export function leadingDocStart(fileLines: string[], startLine: number): number {
  let first = startLine;
  while (
    first > 1 &&
    startLine - first < LEADING_DOC_MAX_LINES &&
    DOC_LINE.test(fileLines[first - 2].trim())
  ) {
    first -= 1;
  }
  return first;
}

/**
 * The node with its line range as it is in the working tree. Indexed lines are
 * trusted while the file is unchanged; otherwise the declaration is re-located
 * by text so reads keep working between edits and the next index refresh.
 */
async function currentRange(
  context: CodeContext,
  fileLines: string[],
  node: GraphNode,
): Promise<{ node: GraphNode; notes: string[] }> {
  if (!(await context.freshness.isStale(node))) return { node, notes: [] };
  const located = relocate(fileLines, node);
  return located === undefined
    ? { node, notes: [STALE_LOST_NOTE] }
    : { node: located, notes: [STALE_NOTE] };
}

function relocate(fileLines: string[], node: GraphNode): GraphNode | undefined {
  const overload = /#(\d+)$/.exec(node.qualifiedName);
  const range = locateDeclaration(fileLines, node.name, node.startLine, {
    type: isTypeNode(node),
    occurrence: overload === null ? undefined : Number(overload[1]),
  });
  return range === undefined ? undefined : { ...node, ...range };
}

/**
 * Members declared outside the type's own body (Rust `impl` blocks, Go methods,
 * out-of-line definitions): the type's source alone would hide them.
 */
async function detachedMembers(context: CodeContext, node: GraphNode): Promise<string[]> {
  const members = (await membersOf(context.client, node.qualifiedName)).filter(
    (member) =>
      !isContainerNode(member) &&
      (member.filePath !== node.filePath ||
        member.startLine < node.startLine ||
        member.endLine > node.endLine),
  );
  if (members.length === 0) return [];

  const rows: string[] = [];
  for (const member of members.slice(0, OUTLINE_MAX_ROWS)) {
    const lines = (await context.reader.lines(member.filePath)) ?? [];
    const row = outlineRow(member.startLine, member.endLine, declarationLine(lines, member));
    rows.push(member.filePath === node.filePath ? row : `${row}\t${member.filePath}`);
  }
  return [`# members defined outside the type body: ${members.length}`, ...rows];
}

export async function renderOutline(context: CodeContext, node: GraphNode): Promise<string> {
  const members = (await membersOf(context.client, node.qualifiedName)).filter(
    (member) => !isContainerNode(member),
  );
  const lines = (await context.reader.lines(node.filePath)) ?? [];
  const stale = await context.freshness.isStale(members[0] ?? node);
  const here = (item: GraphNode) => (stale ? (relocate(lines, item) ?? item) : item);
  const type = here(node);
  const shown = members.slice(0, OUTLINE_MAX_ROWS).map(here);
  const name = displayName(node, context.project);
  return [
    `# outline ${name} ${node.label} ${location(type)} members=${members.length}`,
    ...(stale ? [STALE_NOTE] : []),
    outlineRow(type.startLine, type.startLine, declarationLine(lines, type)),
    ...shown.map((member) => outlineRow(member.startLine, member.endLine, declarationLine(lines, member))),
    ...(members.length > shown.length ? [`# ... ${members.length - shown.length} more members`] : []),
    `# read one: gcal get ${name}.<member> | gcal get ${node.filePath}:<line> | whole: gcal get ${name} --full`,
  ].join("\n");
}

async function readFileLocator(
  context: CodeContext,
  locator: FileLocator,
  options: ReadOptions,
): Promise<ReadResult> {
  const detail = options.detail ?? "auto";
  const resolved = await resolveFilePath(context, locator.path);
  if (typeof resolved !== "string") {
    if (resolved.length > 0) return readAmbiguousFiles(context, locator, resolved);
    return (
      (await readDirectory(context, locator.path)) ?? {
        ok: false,
        text: `# not found: ${locator.raw}`,
      }
    );
  }

  const lines = await context.reader.lines(resolved);
  if (lines === undefined) return { ok: false, text: `# not found: ${locator.raw}` };

  if (locator.startLine !== undefined && locator.endLine !== undefined) {
    return { ok: true, text: renderRange(resolved, lines, locator.startLine, locator.endLine) };
  }

  if (locator.startLine !== undefined) {
    const line = locator.startLine;
    const enclosing = (await nodesAtLine(context.client, resolved, line)).find(
      (node) => !isContainerNode(node),
    );
    // Indexed ranges no longer say which symbol owns this line.
    if (enclosing !== undefined && (await context.freshness.isStale(enclosing))) {
      return {
        ok: true,
        text: [
          STALE_NOTE,
          renderRange(resolved, lines, Math.max(1, line - LINE_WINDOW), line + LINE_WINDOW),
        ].join("\n"),
      };
    }
    const tooLarge =
      enclosing !== undefined &&
      isTypeNode(enclosing) &&
      enclosing.endLine - enclosing.startLine + 1 > FULL_SOURCE_MAX_LINES;
    if (enclosing !== undefined && !tooLarge) {
      return { ok: true, text: await renderSymbol(context, enclosing) };
    }
    return {
      ok: true,
      text: renderRange(resolved, lines, Math.max(1, line - LINE_WINDOW), line + LINE_WINDOW),
    };
  }

  if (detail === "full" || (detail === "auto" && lines.length <= FULL_SOURCE_MAX_LINES)) {
    return {
      ok: true,
      text: [`# ${resolved}:1-${lines.length}`, ...lines].join("\n"),
      compact:
        lines.length > BATCH_OUTLINE_MIN_LINES
          ? () => renderFileOutline(context, resolved, lines)
          : undefined,
    };
  }

  return { ok: true, text: await renderFileOutline(context, resolved, lines) };
}

/**
 * A bare file name shared by a few small files: all of them cost less than
 * the extra call a list of paths would need.
 */
async function readAmbiguousFiles(
  context: CodeContext,
  locator: FileLocator,
  paths: string[],
): Promise<ReadResult> {
  if (locator.startLine === undefined && paths.length <= AMBIGUOUS_INLINE_MAX) {
    const sources: string[][] = [];
    for (const path of paths) sources.push((await context.reader.lines(path)) ?? []);
    const total = sources.reduce((sum, lines) => sum + lines.length, 0);
    if (sources.every((lines) => lines.length > 0) && total <= FULL_SOURCE_MAX_LINES) {
      return {
        ok: true,
        text: [
          `# ${locator.raw} names ${paths.length} files; all shown`,
          ...paths.map((path, index) =>
            [`# ${path}:1-${sources[index].length}`, ...sources[index]].join("\n"),
          ),
        ].join("\n\n"),
      };
    }
  }
  return {
    ok: false,
    text: [`${AMBIGUOUS_FILE} ${locator.raw} names ${paths.length} files; pick one by path`, ...paths].join(
      "\n",
    ),
  };
}

/** `gcal get src/sync`: what the directory holds, so the next call can name files. */
async function readDirectory(context: CodeContext, raw: string): Promise<ReadResult | undefined> {
  const directory = normalizePath(isAbsolute(raw) ? relative(context.root, raw) : raw).replace(
    /\/+$/,
    "",
  );
  if (directory === "") return undefined;

  // `.` is the repository root.
  const prefix = directory === "." ? "" : `${directory}/`;
  const files = ((await context.newFiles?.all()) ?? []).filter((path) => path.startsWith(prefix));
  if (files.length === 0) return undefined;

  const direct: string[] = [];
  const nested = new Map<string, number>();
  for (const path of files) {
    const rest = path.slice(prefix.length);
    const slash = rest.indexOf("/");
    if (slash === -1) direct.push(path);
    else nested.set(rest.slice(0, slash), (nested.get(rest.slice(0, slash)) ?? 0) + 1);
  }

  const rows = [...nested]
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([name, count]) => `${prefix}${name}/\t${count} files`);
  for (const path of direct.sort().slice(0, DIRECTORY_MAX_FILES)) {
    const lines = await context.reader.lines(path);
    rows.push(lines === undefined ? path : `${path}\t${lines.length} lines`);
  }
  return {
    ok: true,
    text: [
      `# directory ${prefix || "./"} files=${files.length}`,
      ...rows,
      ...(direct.length > DIRECTORY_MAX_FILES
        ? [`# ... ${direct.length - DIRECTORY_MAX_FILES} more files`]
        : []),
      `# read: gcal get <file> [<file>...] | search inside: gcal grep <pattern> ${directory}`,
    ].join("\n"),
  };
}

async function renderFileOutline(
  context: CodeContext,
  filePath: string,
  lines: string[],
): Promise<string> {
  const nodes = (await nodesInFile(context.client, filePath)).filter(
    (node) => !isContainerNode(node),
  );
  if (nodes.length === 0) {
    // Nothing indexed to outline (config, docs, unsupported language).
    if (lines.length <= FULL_SOURCE_MAX_LINES) {
      return [`# ${filePath}:1-${lines.length}`, ...lines].join("\n");
    }
    return [
      `# ${filePath}:1-${FULL_SOURCE_MAX_LINES} of ${lines.length} lines`,
      ...lines.slice(0, FULL_SOURCE_MAX_LINES),
      `# ... more: gcal get ${filePath}:${FULL_SOURCE_MAX_LINES + 1}-${lines.length}`,
    ].join("\n");
  }

  const stale = await context.freshness.isStale(nodes[nodes.length - 1]);
  const shown = nodes
    .slice(0, OUTLINE_MAX_ROWS)
    .map((node) => (stale ? (relocate(lines, node) ?? node) : node));
  return [
    `# outline ${filePath} lines=${lines.length} symbols=${nodes.length}`,
    ...(stale ? [STALE_NOTE] : []),
    ...shown.map((node) => outlineRow(node.startLine, node.endLine, declarationLine(lines, node))),
    ...(nodes.length > shown.length ? [`# ... ${nodes.length - shown.length} more symbols`] : []),
    `# read part: gcal get ${filePath}:<line> | gcal get ${filePath}:<start>-<end> | whole: gcal get ${filePath} --full`,
  ].join("\n");
}

export async function resolveFilePath(context: CodeContext, path: string): Promise<string | string[]> {
  let candidate = path;
  if (isAbsolute(candidate)) {
    candidate = normalizePath(relative(context.root, candidate));
  }
  if ((await context.reader.lines(candidate)) !== undefined) return candidate;

  const indexed = await filesBySuffix(context.client, candidate);
  // The working tree also holds files the index does not: new ones, build
  // scripts, languages without a grammar.
  const present = ((await context.newFiles?.all()) ?? []).filter(
    (filePath) => filePath === candidate || filePath.endsWith(`/${candidate}`),
  );
  const matches = [...new Set([...indexed, ...present])];
  return matches.length === 1 ? matches[0] : matches;
}

async function renderAmbiguous(
  context: CodeContext,
  locator: SymbolLocator,
  nodes: GraphNode[],
): Promise<ReadResult> {
  const small = nodes.every(
    (node) => node.endLine - node.startLine + 1 <= AMBIGUOUS_INLINE_MAX_LINES,
  );
  if (nodes.length <= AMBIGUOUS_INLINE_MAX && small) {
    const blocks = [];
    for (const node of nodes) blocks.push(await renderSymbol(context, node));
    return { ok: true, text: blocks.join("\n\n") };
  }

  return {
    ok: true,
    text: [
      `# ambiguous: ${locator.raw} matches ${nodes.length} symbols; pick one with gcal get <file>:<line>`,
      ...nodes.map((node) => symbolRow(context, node)),
    ].join("\n"),
  };
}

function renderRange(filePath: string, lines: string[], startLine: number, endLine: number): string {
  const start = Math.max(1, Math.min(startLine, lines.length));
  const requestedEnd = Math.max(start, Math.min(endLine, lines.length));
  const end = Math.min(requestedEnd, start + RANGE_MAX_LINES - 1);
  return [
    `# ${filePath}:${start}-${end} of ${lines.length} lines`,
    ...lines.slice(start - 1, end),
    ...(end < requestedEnd ? [`# ... more: gcal get ${filePath}:${end + 1}-${requestedEnd}`] : []),
  ].join("\n");
}

async function backendSource(context: CodeContext, node: GraphNode): Promise<string> {
  try {
    const raw = await context.client.tool("get_code_snippet", {
      qualified_name: node.qualifiedName,
    });
    if (typeof raw === "object" && raw !== null) {
      const record = raw as Record<string, unknown>;
      const source = record.code ?? record.source;
      if (typeof source === "string") return source;
    }
  } catch (error) {
    return `# source unavailable: ${error instanceof Error ? error.message : String(error)}`;
  }
  return "# source unavailable";
}

export function symbolRow(context: CodeContext, node: GraphNode): string {
  return [displayName(node, context.project), node.label, location(node)].join("\t");
}

/** The line that declares the symbol: skips comments, annotations and attributes. */
export function declarationLine(fileLines: string[], node: GraphNode): string {
  const line = declarationLineNumber(fileLines, node);
  return compactDeclaration(line === undefined ? node.name : fileLines[line - 1].trim());
}

/** 1-based line number of the declaration, or undefined when the file has no such line. */
export function declarationLineNumber(fileLines: string[], node: GraphNode): number | undefined {
  const end = Math.min(node.endLine, node.startLine + 40, fileLines.length);
  let fallback: number | undefined;
  for (let index = Math.max(0, node.startLine - 1); index < end; index += 1) {
    const text = fileLines[index].trim();
    if (text.length === 0 || /^(\/\/|\/\*|\*|#\[|#!\[|"""|''')/.test(text)) continue;
    fallback ??= index + 1;
    if (text.includes(node.name)) return index + 1;
  }
  return fallback;
}

function compactDeclaration(text: string): string {
  const compact = text.replace(/\s+/g, " ").replace(/\s*\{$/, "");
  return compact.length > DECLARATION_MAX_CHARS
    ? `${compact.slice(0, DECLARATION_MAX_CHARS - 1)}…`
    : compact;
}

function outlineRow(startLine: number, endLine: number, declaration: string): string {
  return `${endLine > startLine ? `${startLine}-${endLine}` : String(startLine)}\t${declaration}`;
}

function byteLength(text: string): number {
  return Buffer.byteLength(text, "utf8");
}

function totalBytes(texts: string[]): number {
  return texts.reduce((sum, text) => sum + byteLength(text), 0);
}

/**
 * Splits the budget so small items keep their full size and only the largest
 * ones share what is left.
 */
export function fairBudgets(sizes: number[], total: number): number[] {
  const budgets = new Array<number>(sizes.length).fill(0);
  const ascending = sizes.map((size, index) => ({ size, index })).sort((a, b) => a.size - b.size);
  let remaining = total;
  ascending.forEach(({ size, index }, position) => {
    const share = Math.floor(remaining / (ascending.length - position));
    budgets[index] = Math.min(size, share);
    remaining -= budgets[index];
  });
  return budgets;
}

function truncateBytes(text: string, maxBytes: number): string {
  if (Buffer.byteLength(text, "utf8") <= maxBytes) return text;
  const marker = "\n# ... truncated; request fewer items or a line range";
  let end = Math.max(0, maxBytes - Buffer.byteLength(marker, "utf8"));
  let prefix = Buffer.from(text, "utf8").subarray(0, end).toString("utf8");
  // Drop a trailing partial character and the partial last line.
  prefix = prefix.replace(/�$/, "");
  end = prefix.lastIndexOf("\n");
  return `${end > 0 ? prefix.slice(0, end) : prefix}${marker}`;
}
