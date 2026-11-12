import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { isAbsolute, join, relative } from "node:path";
import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import type { SymbolCandidate, TraceEdge } from "../domain/types.js";
import {
  FILE_LIST_MAX_ROWS,
  allFilePaths,
  findByExactName,
  isContainerNode,
  isTypeNode,
  nodesAtLine,
  subtypesOf,
  textSearch,
  type GraphNode,
  type TextMatch,
} from "../kernel/codeGraph.js";
import { displayName, isTestPath } from "../kernel/displayName.js";
import { normalizePath, parseLocator, parseSymbolLocator } from "../kernel/locator.js";
import {
  declarationLine,
  declarationLineNumber,
  renderSymbol,
  resolveFilePath,
  symbolRow,
  type CodeContext,
  type ReadResult,
} from "./readCode.js";
import { resolveSymbol } from "./resolveSymbol.js";

const SNIPPET_MAX_LINES = 60;
const TEXT_LINE_MAX_CHARS = 160;
const TEXT_REFERENCE_LIMIT = 80;
const ARCH_MAX_ROWS = 40;
const MAX_SEARCH_ROWS = 40;
const SEARCH_OVERFETCH_FACTOR = 3;
const TEXT_LINES_SHOWN = 5;
// The backend ranks before it truncates; over-fetch so definitions and
// production code can be ordered first locally.
const TEXT_OVERFETCH_FACTOR = 3;
const TEXT_BACKEND_MAX = 120;
const CONTEXT_MAX_LINES = 400;
const CONTEXT_LINE_MAX_CHARS = 240;
const LOCAL_SCAN_MAX_FILES = 2000;
const LOCAL_SCAN_MAX_LINES = 20_000;
export const BINARY_FILE =
  /\.(png|jpe?g|gif|ico|webp|bmp|pdf|zip|gz|tgz|7z|jar|class|exe|dll|so|dylib|bin|woff2?|ttf|otf|mp[34]|mov|db|sqlite|lock)$/i;

export interface SearchRequest {
  queries: string[];
  limit: number;
  label?: string;
  filePattern?: string;
  qualifiedNamePattern?: string;
  /** Number of top symbols whose source is appended. */
  snippets?: number;
}

export interface TextRequest {
  pattern: string;
  /** Read the pattern as a regular expression first, as literal text when that finds nothing. */
  regex: boolean;
  filePattern?: string;
  /** Files or directories to search in, as given to rg. */
  paths?: string[];
  /** Lines of context around each match; prints source lines instead of one row per symbol. */
  context?: number;
  limit: number;
}

export interface ReferenceRequest {
  target: string;
  depth: number;
  limit: number;
  includeText: boolean;
}

export async function searchCode(
  context: CodeContext,
  backend: GcalBackendClient,
  request: SearchRequest,
): Promise<ReadResult> {
  const sections: string[] = [];
  // One entry per symbol; overloads of it share the entry.
  const shown: GraphNode[][] = [];
  const seen = new Set<string>();
  const emptyQueries: string[] = [];

  // The limit applies to each query, so a later query is never starved.
  for (const query of request.queries) {
    const candidates = await backend.search(query, {
      limit: request.limit * SEARCH_OVERFETCH_FACTOR + 6,
      label: request.label,
      filePattern: request.filePattern,
      qualifiedNamePattern: request.qualifiedNamePattern,
    });
    const groups = groupOverloads(rankCandidates(query, candidates.map(candidateNode), request.label));
    if (groups.length === 0) emptyQueries.push(query);
    let taken = 0;
    for (const group of groups) {
      if (taken >= request.limit || shown.length >= MAX_SEARCH_ROWS) break;
      const base = baseName(group[0]);
      if (seen.has(base)) continue;
      seen.add(base);
      shown.push(group);
      taken += 1;
    }
  }

  if (shown.length > 0) {
    sections.push((await Promise.all(shown.map((group) => searchRow(context, group)))).join("\n"));
  }

  // No symbol is named like the query: it is probably text (a literal, a
  // keyword phrase, a config key), so answer that in the same call.
  for (const query of emptyQueries) {
    const text = await grepCode(context, {
      pattern: query,
      regex: true,
      filePattern: request.filePattern,
      limit: request.limit,
    });
    sections.push(`# no symbol named ${JSON.stringify(query)}; text matches:\n${text.text}`);
  }

  const snippetCount = request.snippets ?? (shown.length === 1 && emptyQueries.length === 0 ? 1 : 0);
  for (const group of shown.slice(0, snippetCount)) {
    for (const node of group) {
      const [exact] = await findByExactName(context.client, node.qualifiedName);
      if (exact !== undefined) sections.push(capLines(await renderSymbol(context, exact)));
    }
  }

  return { text: sections.join("\n\n"), ok: true };
}

export async function grepCode(context: CodeContext, request: TextRequest): Promise<ReadResult> {
  // `.` is the whole repository, as in `rg pattern .`.
  const paths = separatePaths(context.root, request.paths ?? []).filter(
    (path) => !/^\.?[\\/]*$/.test(path),
  );
  const pathFilter = paths.length > 0 ? pathsPattern(context.root, paths) : request.filePattern;
  const found = await findText(context, request.pattern, request.regex, {
    pathFilter,
    limit: Math.min(TEXT_BACKEND_MAX, request.limit * TEXT_OVERFETCH_FACTOR),
  });
  const unindexed = (
    await unindexedMatches(context, request.pattern, request.regex, pathFilter, paths.length > 0)
  ).filter((local) => !found.matches.some((match) => match.filePath === local.filePath));
  const everything = [...unindexed, ...found.matches];
  // Paths went to the backend; a file pattern given with them narrows further here.
  const alsoFile =
    paths.length > 0 && request.filePattern !== undefined ? new RegExp(request.filePattern) : undefined;
  const matches =
    alsoFile === undefined ? everything : everything.filter((match) => alsoFile.test(match.filePath));
  const total = alsoFile === undefined ? found.total + unindexed.length : matches.length;
  if (matches.length === 0) {
    // `\\b` typed inside double quotes reaches here as two backslashes.
    const single = request.regex ? undoubled(request.pattern) : undefined;
    if (single !== undefined) {
      const retry = await grepCode(context, { ...request, pattern: single });
      if (retry.text !== NO_TEXT_MATCHES) {
        return { ok: true, text: `${undoubledNote(request.pattern, single)}\n${retry.text}` };
      }
    }
    return { text: NO_TEXT_MATCHES, ok: true };
  }

  const shown = sortMatches(matches).slice(0, request.limit);
  const hidden = Math.max(total, matches.length) - shown.length;
  const hiddenFiles = (printed: Set<string>) =>
    filesNotShown(context, printed, {
      pattern: request.pattern,
      // Matches that came only from the local scan were found as a regex.
      literal: !request.regex || (found.literal && found.matches.length > 0),
      pathFilter,
      scoped: paths.length > 0,
      alsoFile,
    });
  if (request.context !== undefined) {
    const blocks = await contextBlocks(context, shown, request.context);
    const lines = blocks.lines;
    if (hidden > 0 || blocks.skipped > 0) {
      if (hidden > 0) lines.push(`# ... ${hidden} more matches not shown; give paths or raise --limit`);
      lines.push(...(await hiddenFiles(blocks.printed)));
    }
    return { text: lines.join("\n"), ok: true };
  }

  const rows: FileRow[] = [];
  for (const match of shown) {
    rows.push({
      filePath: match.filePath,
      fields: [
        lineList(match.lines),
        match.enclosing === undefined ? "" : displayName(match.enclosing, context.project),
        await lineText(context, match.filePath, match.lines[0]),
      ],
    });
  }
  const lines = groupByFile(rows);
  if (hidden > 0) {
    lines.push(`# ... ${hidden} more matches not shown; give paths or raise --limit`);
    lines.push(...(await hiddenFiles(new Set(shown.map((match) => match.filePath)))));
  }
  return { text: lines.join("\n"), ok: true };
}

/**
 * A PowerShell array given to a script arrives as one argument, its paths
 * joined by spaces. An argument that is no path itself but splits into paths
 * that exist is that list.
 */
export function separatePaths(root: string, paths: string[]): string[] {
  const exists = (path: string) => existsSync(isAbsolute(path) ? path : join(root, path));
  return paths.flatMap((path) => {
    if (!/\s/.test(path) || exists(path)) return [path];
    const parts = path.split(/\s+/).filter((part) => part.length > 0);
    return parts.length > 1 && parts.every(exists) ? parts : [path];
  });
}

/** A file-path regex for an rg-style glob: `*.py`, `src/**`, `tests/*.{ts,tsx}`. */
export function globPattern(glob: string): string {
  let pattern = "";
  for (let index = 0; index < glob.length; index += 1) {
    const char = glob[index];
    if (char === "*" && glob[index + 1] === "*") {
      pattern += ".*";
      index += glob[index + 2] === "/" ? 2 : 1;
    } else if (char === "*") pattern += "[^/]*";
    else if (char === "?") pattern += "[^/]";
    else if (char === "{") pattern += "(";
    else if (char === "}") pattern += ")";
    else if (char === ",") pattern += "|";
    else pattern += escapeForRegex(char === "\\" ? "/" : char);
  }
  return `(^|/)${pattern}$`;
}

const VALUE_LABELS = new Set(["Variable", "Field", "Constant", "Property"]);
const LANGUAGE_FAMILIES = [
  ["ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "vue", "svelte"],
  ["java", "kt", "kts", "scala", "groovy"],
  ["py", "pyi"],
  ["c", "h", "cc", "cpp", "cxx", "hh", "hpp", "hxx", "m", "mm"],
];

/** A path filter for the files that can use a symbol declared in `filePath`. */
export function sameLanguageFilter(filePath: string): string | undefined {
  const extension = /\.([A-Za-z0-9]+)$/.exec(filePath)?.[1]?.toLowerCase();
  if (extension === undefined) return undefined;
  const family = LANGUAGE_FAMILIES.find((group) => group.includes(extension)) ?? [extension];
  return `\\.(${family.join("|")})$`;
}
const NO_TEXT_MATCHES = "# no text matches";
const HIDDEN_SCAN_MAX_FILES = 20_000;
const HIDDEN_FILES_SHOWN = 60;
const HIDDEN_SCAN_BATCH = 32;
const HIDDEN_SCAN_MAX_CHARS = 2 * 1024 * 1024;

/** The pattern with each doubled backslash halved, when it has any. */
export function undoubled(pattern: string): string | undefined {
  return pattern.includes("\\\\") ? pattern.replaceAll("\\\\", "\\") : undefined;
}

export function undoubledNote(pattern: string, single: string): string {
  return `# no match for ${pattern}; read as ${single} (doubled backslashes)`;
}

/**
 * A cut search must still say which files match: an edit planned from the
 * output would otherwise skip the files that were cut. The backend returns
 * only the rows it was asked for, so the remaining files are found by a local
 * scan of the same scope.
 */
async function filesNotShown(
  context: CodeContext,
  printed: Set<string>,
  search: {
    pattern: string;
    literal: boolean;
    pathFilter: string | undefined;
    scoped: boolean;
    alsoFile: RegExp | undefined;
  },
): Promise<string[]> {
  if (context.newFiles === undefined) return [];
  const matcher = textMatcher(search.pattern, search.literal);
  let filter: RegExp | undefined;
  try {
    filter = search.pathFilter === undefined ? undefined : new RegExp(search.pathFilter);
  } catch {
    return [];
  }
  if (matcher === undefined) return [];

  let files = (await context.newFiles.all()).filter(
    (path) =>
      !printed.has(path) &&
      !BINARY_FILE.test(path) &&
      (filter === undefined || filter.test(path)) &&
      (search.alsoFile === undefined || search.alsoFile.test(path)),
  );
  if (!search.scoped) {
    // Without paths the search covers the index and the files added since.
    const indexed = await allFilePaths(context.client);
    if (indexed.length < FILE_LIST_MAX_ROWS) {
      const known = new Set([...indexed, ...(await context.newFiles.list())]);
      files = files.filter((path) => known.has(path));
    }
  }
  if (files.length > HIDDEN_SCAN_MAX_FILES) return [];

  const counts: Array<[string, number]> = [];
  for (let start = 0; start < files.length; start += HIDDEN_SCAN_BATCH) {
    const batch = files.slice(start, start + HIDDEN_SCAN_BATCH);
    const scanned = await Promise.all(
      batch.map(async (filePath): Promise<[string, number]> => {
        let text: string;
        try {
          text = await readFile(join(context.root, filePath), "utf8");
        } catch {
          return [filePath, 0];
        }
        // Most files do not match; one pass over the text settles those.
        if (text.length > HIDDEN_SCAN_MAX_CHARS || !matcher.text(text)) return [filePath, 0];
        return [filePath, text.split("\n").reduce((sum, line) => sum + Number(matcher.line(line)), 0)];
      }),
    );
    counts.push(...scanned.filter(([, count]) => count > 0));
  }
  if (counts.length === 0) return [];

  // Files of one directory share its path: `dir/{A.java 2, B.java 1}`.
  const byDirectory = new Map<string, string[]>();
  for (const [filePath, count] of counts.slice(0, HIDDEN_FILES_SHOWN)) {
    const cut = filePath.lastIndexOf("/") + 1;
    const directory = filePath.slice(0, cut);
    const names = byDirectory.get(directory) ?? [];
    names.push(`${filePath.slice(cut)} ${count}`);
    byDirectory.set(directory, names);
  }
  const listed = [...byDirectory]
    .map(([directory, names]) =>
      names.length === 1 ? `${directory}${names[0]}` : `${directory}{${names.join(", ")}}`,
    )
    .join(", ");
  const more = counts.length > HIDDEN_FILES_SHOWN ? `, +${counts.length - HIDDEN_FILES_SHOWN} more files` : "";
  return [`# matching files not shown (path matching-lines): ${listed}${more}`];
}

interface TextMatcher {
  /** False when no line of the text can match. */
  text: (text: string) => boolean;
  line: (line: string) => boolean;
}

function textMatcher(pattern: string, literal: boolean): TextMatcher | undefined {
  if (literal) {
    const includes = (text: string) => text.includes(pattern);
    return { text: includes, line: includes };
  }
  // rg spells case-insensitivity inside the pattern.
  const insensitive = pattern.startsWith("(?i)");
  const source = insensitive ? pattern.slice(4) : pattern;
  try {
    const whole = new RegExp(source, insensitive ? "im" : "m");
    const single = new RegExp(source, insensitive ? "i" : "");
    return { text: (text) => whole.test(text), line: (line) => single.test(line) };
  } catch {
    return undefined;
  }
}

export async function references(
  context: CodeContext,
  backend: GcalBackendClient,
  request: ReferenceRequest,
): Promise<ReadResult> {
  const resolved = await resolveTarget(context, request.target);
  if (!Array.isArray(resolved)) return resolved;

  const definitions = new Set(resolved.map((node) => node.qualifiedName));
  const rows = new Map<string, ReferenceRow>();
  for (const node of resolved) {
    let edges: TraceEdge[];
    try {
      edges = await backend.callers(node.qualifiedName, { depth: request.depth });
    } catch (error) {
      // The call graph does not hold this symbol (a constant, a variable);
      // its uses are then found as text below.
      if (!(error instanceof Error && /not found/i.test(error.message))) throw error;
      edges = [];
    }
    for (const edge of edges) {
      if (definitions.has(edge.relatedQualifiedName)) continue;
      const existing = rows.get(edge.relatedQualifiedName);
      if (existing === undefined) rows.set(edge.relatedQualifiedName, graphRow(context, edge));
    }
  }

  let textOnly = 0;
  if (request.includeText && request.depth === 1) {
    const name = resolved[0].name;
    // A callable is used where it is called; a type or a value wherever it is named.
    const isValue = VALUE_LABELS.has(resolved[0].label);
    const pattern =
      isTypeNode(resolved[0]) || isValue
        ? `\\b${escapeForRegex(name)}\\b`
        : `\\b${escapeForRegex(name)}\\s*\\(`;
    const { matches } = await textSearch(context.client, pattern, {
      regex: true,
      limit: TEXT_REFERENCE_LIMIT,
      // The bare name of a value is often an ordinary word in prose and
      // configuration; its uses are in code of its own language.
      pathFilter: isValue ? sameLanguageFilter(resolved[0].filePath) : undefined,
    });
    for (const match of matches) {
      const enclosing = match.enclosing;
      if (enclosing !== undefined && definitions.has(enclosing.qualifiedName)) continue;

      // Inside a same-named symbol (an override, an interface declaration) the
      // declaration line itself is not a call site; a delegating call below it is.
      let lines = match.lines;
      if (enclosing !== undefined && enclosing.name === name) {
        const fileLines = (await context.reader.lines(match.filePath)) ?? [];
        const declared = declarationLineNumber(fileLines, enclosing);
        lines = lines.filter((line) => line !== declared);
      }
      if (lines.length === 0) continue;

      const key = enclosing?.qualifiedName ?? `${match.filePath}:${lines[0]}`;
      const callSite = await lineText(context, match.filePath, lines[0]);
      const existing = rows.get(key);
      if (existing !== undefined) {
        existing.lines = lines;
        existing.callSite = callSite;
        continue;
      }
      textOnly += 1;
      rows.set(key, {
        name: enclosing === undefined ? "" : displayName(enclosing, context.project),
        filePath: match.filePath,
        lines,
        kind: "text",
        hop: 1,
        callSite,
      });
    }
  }

  const ordered = [...rows.values()].sort(
    (left, right) =>
      left.hop - right.hop ||
      Number(isTestPath(left.filePath)) - Number(isTestPath(right.filePath)) ||
      left.filePath.localeCompare(right.filePath),
  );
  const shown = ordered.slice(0, request.limit);
  const target = displayName(resolved[0], context.project);
  const lines = [
    `# callers of ${target}: ${ordered.length} (graph ${ordered.length - textOnly}, text-only ${textOnly})`,
    ...groupByFile(
      shown.map((row) => ({
        filePath: row.filePath,
        fields: [lineList(row.lines), row.name, row.hop > 1 ? `hop ${row.hop}` : row.kind, row.callSite],
      })),
    ),
  ];
  if (ordered.length > shown.length) {
    const hiddenTests = ordered.slice(request.limit).filter((row) => isTestPath(row.filePath)).length;
    lines.push(
      `# ... ${ordered.length - shown.length} more (${hiddenTests} in tests); rerun with --limit ${ordered.length}`,
    );
  }
  return { text: lines.join("\n"), ok: true };
}

export async function callees(
  context: CodeContext,
  backend: GcalBackendClient,
  request: Omit<ReferenceRequest, "includeText">,
): Promise<ReadResult> {
  const resolved = await resolveTarget(context, request.target);
  if (!Array.isArray(resolved)) return resolved;

  const rows = new Map<string, ReferenceRow>();
  for (const node of resolved) {
    for (const edge of await backend.callees(node.qualifiedName, { depth: request.depth })) {
      if (!rows.has(edge.relatedQualifiedName)) {
        rows.set(edge.relatedQualifiedName, graphRow(context, edge));
      }
    }
  }

  const ordered = [...rows.values()].sort((left, right) => left.hop - right.hop);
  const shown = ordered.slice(0, request.limit);
  const lines = [
    `# callees of ${displayName(resolved[0], context.project)}: ${ordered.length}`,
    ...shown.map((row) =>
      [row.name, `${row.filePath}:${row.lines[0]}`, request.depth > 1 ? `hop ${row.hop}` : ""]
        .filter((field) => field.length > 0)
        .join("\t"),
    ),
  ];
  if (ordered.length > shown.length) {
    lines.push(`# ... ${ordered.length - shown.length} more; rerun with --limit ${ordered.length}`);
  }
  return { text: lines.join("\n"), ok: true };
}

const TESTS_SEARCH_LIMIT = 60;
const TESTS_SOURCE_SHOWN = 4;
const TESTS_SOURCE_MAX_LINES = 40;
const TESTS_ROWS_SHOWN = 12;
const TESTS_LIFTED = 24;
// The backend narrows by path first; isTestPath then decides.
const TEST_PATH_FILTER =
  "(?i)((^|/)(tests?|__tests__|specs?|testing|testdata|fixtures)/|(tests?|spec|it)\\.[a-z]+$)";

/**
 * The tests that use a symbol, for `gcal get --tests`: a change to a symbol
 * usually comes with a change to its tests, and reading them otherwise takes a
 * second call. The first few are printed whole, the rest as rows.
 */
export async function testsUsing(context: CodeContext, raw: string): Promise<string | undefined> {
  const place = parseLocator(raw);
  if (place.kind === "file" && place.startLine === undefined) return undefined;
  const resolved = await resolveTarget(context, raw);
  if (!Array.isArray(resolved)) return undefined;

  const target = resolved[0];
  const definitions = new Set(resolved.map((node) => node.qualifiedName));
  const pattern =
    isTypeNode(target) || VALUE_LABELS.has(target.label)
      ? `\\b${escapeForRegex(target.name)}\\b`
      : `\\b${escapeForRegex(target.name)}\\s*\\(`;
  const { matches } = await textSearch(context.client, pattern, {
    regex: true,
    limit: TESTS_SEARCH_LIMIT,
    pathFilter: TEST_PATH_FILTER,
  });

  // Tests of the symbol's own file come first: `StringUtilsTests` for `StringUtils`.
  const stem = (target.filePath.split("/").pop() ?? "").replace(/\.[^.]+$/, "");
  const near = (filePath: string) => Number(!(filePath.split("/").pop() ?? "").includes(stem));
  const ordered = matches
    .filter(
      (match) =>
        isTestPath(match.filePath) &&
        (match.enclosing === undefined || !definitions.has(match.enclosing.qualifiedName)),
    )
    .map((match, index) => ({ match, index }))
    .sort((left, right) => near(left.match.filePath) - near(right.match.filePath) || left.index - right.index)
    .map((entry) => entry.match);

  // A use inside a lambda or a local function belongs to the test around it:
  // that test is what a reader wants, once.
  const uses: TextMatch[] = [];
  const byTest = new Map<string, TextMatch>();
  for (const [index, match] of ordered.entries()) {
    const test = index < TESTS_LIFTED ? await outermostCallable(context, match) : match.enclosing;
    if (test === undefined) {
      uses.push(match);
      continue;
    }
    const known = byTest.get(test.qualifiedName);
    if (known !== undefined) {
      known.lines = [...new Set([...known.lines, ...match.lines])].sort((left, right) => left - right);
      continue;
    }
    const use = { ...match, enclosing: test };
    byTest.set(test.qualifiedName, use);
    uses.push(use);
  }

  const name = displayName(target, context.project);
  if (uses.length === 0) return `# tests using ${name}: none found`;

  const files = new Set(uses.map((match) => match.filePath));
  const sections = [
    `# tests using ${name}: ${uses.length} in ${files.size} file${files.size === 1 ? "" : "s"}`,
  ];
  const rows: FileRow[] = [];
  let printed = 0;
  for (const match of uses) {
    const enclosing = match.enclosing;
    const whole =
      enclosing !== undefined &&
      !isTypeNode(enclosing) &&
      enclosing.endLine - enclosing.startLine + 1 <= TESTS_SOURCE_MAX_LINES;
    if (whole && printed < TESTS_SOURCE_SHOWN) {
      printed += 1;
      sections.push(await renderSymbol(context, enclosing));
      continue;
    }
    if (rows.length < TESTS_ROWS_SHOWN) {
      rows.push({
        filePath: match.filePath,
        fields: [
          lineList(match.lines),
          enclosing === undefined ? "" : displayName(enclosing, context.project),
          await lineText(context, match.filePath, match.lines[0]),
        ],
      });
    }
  }
  const listed = printed + rows.length;
  if (rows.length > 0) sections.push(groupByFile(rows).join("\n"));
  if (uses.length > listed) sections.push(`# ... ${uses.length - listed} more: gcal callers ${raw}`);
  return sections.join("\n\n");
}

/** The widest function or method around a match, or its indexed enclosing symbol. */
async function outermostCallable(context: CodeContext, match: TextMatch): Promise<GraphNode | undefined> {
  let atLine: GraphNode[];
  try {
    atLine = await nodesAtLine(context.client, match.filePath, match.lines[0]);
  } catch {
    // The backend refuses this query on very large indexes; the symbol the
    // search reported is then the best answer.
    return match.enclosing;
  }
  const around = atLine.filter(
    (node) => !isContainerNode(node) && !isTypeNode(node) && !VALUE_LABELS.has(node.label),
  );
  if (around.length === 0) return match.enclosing;
  return around.reduce((widest, node) =>
    node.endLine - node.startLine > widest.endLine - widest.startLine ? node : widest,
  );
}

export async function implementations(
  context: CodeContext,
  target: string,
  depth: number,
): Promise<ReadResult> {
  const resolved = await resolveTarget(context, target);
  if (!Array.isArray(resolved)) return resolved;

  const rows: string[] = [];
  const visited = new Set(resolved.map((node) => node.qualifiedName));
  let frontier = resolved;
  for (let level = 1; level <= depth && frontier.length > 0; level += 1) {
    const next: GraphNode[] = [];
    for (const parent of frontier) {
      for (const subtype of await subtypesOf(context.client, parent.qualifiedName)) {
        if (visited.has(subtype.node.qualifiedName)) continue;
        visited.add(subtype.node.qualifiedName);
        next.push(subtype.node);
        rows.push(
          [
            symbolRow(context, subtype.node),
            `${subtype.relation.toLowerCase()} ${displayName(parent, context.project)}`,
          ].join("\t"),
        );
      }
    }
    frontier = next;
  }

  const name = displayName(resolved[0], context.project);
  if (rows.length > 0) {
    return { text: [`# subtypes of ${name}: ${rows.length}`, ...rows].join("\n"), ok: true };
  }

  const text = await grepCode(context, {
    pattern: subtypeTextPattern(resolved[0].name),
    regex: true,
    limit: 30,
  });
  return { text: `# subtypes of ${name}: none in graph; text matches:\n${text.text}`, ok: true };
}

/** Declarations that name a supertype: Java/TS keywords, Rust `impl .. for`, Python bases. */
export function subtypeTextPattern(name: string): string {
  const escaped = escapeForRegex(name);
  return [
    `\\b(implements|extends|impl)\\b[^{;]*\\b${escaped}\\b`,
    `\\bclass\\s+\\w+\\s*\\([^)]*\\b${escaped}\\b`,
  ].join("|");
}

export async function architecture(context: CodeContext): Promise<ReadResult> {
  const paths = await allFilePaths(context.client);
  const note =
    paths.length >= FILE_LIST_MAX_ROWS
      ? `\n# partial: only the first ${FILE_LIST_MAX_ROWS} indexed files are counted`
      : "";
  return { text: `${summarizeLayout(paths)}${note}`, ok: true };
}

export function summarizeLayout(paths: string[], maxRows = ARCH_MAX_ROWS): string {
  interface Dir {
    path: string;
    direct: number;
    total: number;
    children: Map<string, Dir>;
  }
  const root: Dir = { path: "", direct: 0, total: 0, children: new Map() };
  const extensions = new Map<string, number>();

  for (const filePath of paths) {
    const parts = filePath.split("/");
    const fileName = parts.pop() ?? "";
    const extension = /\.([A-Za-z0-9]+)$/.exec(fileName)?.[1] ?? "(none)";
    extensions.set(extension, (extensions.get(extension) ?? 0) + 1);
    let dir = root;
    dir.total += 1;
    for (const part of parts) {
      let child = dir.children.get(part);
      if (child === undefined) {
        child = { path: dir.path.length === 0 ? part : `${dir.path}/${part}`, direct: 0, total: 0, children: new Map() };
        dir.children.set(part, child);
      }
      child.total += 1;
      dir = child;
    }
    dir.direct += 1;
  }

  // A directory holding nothing but one subdirectory is just a longer path.
  const collapse = (dir: Dir): Dir => {
    let current = dir;
    while (current.direct === 0 && current.children.size === 1) {
      current = [...current.children.values()][0];
    }
    return current;
  };

  // Expand the largest production directories first; a directory whose
  // children do not fit keeps its biggest ones and folds the rest into one row.
  const minExpandable = Math.max(3, Math.floor(paths.length * 0.02));
  let rows: Dir[] = [...root.children.values()].map(collapse);
  const leaves = new Set<Dir>();
  while (rows.length < maxRows) {
    const expandable = rows
      .filter((dir) => !leaves.has(dir) && dir.children.size > 0 && dir.total >= minExpandable)
      .sort(
        (left, right) =>
          Number(isTestPath(`${left.path}/`)) - Number(isTestPath(`${right.path}/`)) ||
          right.total - left.total,
      )[0];
    if (expandable === undefined) break;

    const own: Dir[] =
      expandable.direct > 0
        ? [{ path: `${expandable.path}/*`, direct: expandable.direct, total: expandable.direct, children: new Map() }]
        : [];
    const slots = maxRows - (rows.length - 1) - own.length;
    if (slots < 2) {
      leaves.add(expandable);
      continue;
    }

    let children = [...expandable.children.values()]
      .map(collapse)
      .sort((left, right) => right.total - left.total);
    if (children.length > slots) {
      const folded = children.slice(slots - 1);
      children = children.slice(0, slots - 1).concat({
        path: `${expandable.path}/(${folded.length} more dirs)`,
        direct: 0,
        total: folded.reduce((sum, dir) => sum + dir.total, 0),
        children: new Map(),
      });
    }
    for (const dir of own) leaves.add(dir);
    rows = rows.filter((dir) => dir !== expandable).concat(children, own);
  }

  const languages = [...extensions.entries()]
    .sort((left, right) => right[1] - left[1])
    .slice(0, 8)
    .map(([extension, count]) => `${extension} ${count}`)
    .join(", ");

  return [
    `# layout: ${paths.length} indexed files (${languages})`,
    ...(root.direct > 0 ? [`${root.direct}\t./*`] : []),
    ...rows
      .sort((left, right) => left.path.localeCompare(right.path))
      .map((dir) => `${dir.total}\t${dir.path}${/(\/\*|\))$/.test(dir.path) ? "" : "/"}`),
  ].join("\n");
}

interface ReferenceRow {
  name: string;
  filePath: string;
  lines: number[];
  kind: "call" | "text";
  hop: number;
  callSite: string;
}

function graphRow(context: CodeContext, edge: TraceEdge): ReferenceRow {
  return {
    name: displayName(edgeNode(edge), context.project),
    filePath: edge.filePath,
    lines: [edge.line ?? 0],
    kind: "call",
    hop: edge.hop ?? 1,
    callSite: "",
  };
}

async function resolveTarget(context: CodeContext, target: string): Promise<GraphNode[] | ReadResult> {
  // `File.ext:42` is the symbol that contains that line.
  const place = parseLocator(target);
  if (place.kind === "file" && place.startLine !== undefined && place.endLine === undefined) {
    const filePath = await resolveFilePath(context, place.path);
    if (typeof filePath === "string") {
      const enclosing = (await nodesAtLine(context.client, filePath, place.startLine)).find(
        (node) => !isContainerNode(node),
      );
      if (enclosing !== undefined) return [enclosing];
    }
    return { ok: false, text: `# not found: no indexed symbol at ${place.raw}` };
  }

  const locator = parseSymbolLocator(target);
  const resolution = await resolveSymbol(context.client, context.project, locator);
  if (resolution.kind === "resolved") return resolution.nodes;

  if (resolution.kind === "ambiguous") {
    return {
      ok: false,
      text: [
        `# ambiguous: ${locator.raw} matches ${resolution.nodes.length} symbols; name its owner (Type.member, file.ext.member) or give its path:line`,
        ...resolution.nodes.map((node) => symbolRow(context, node)),
      ].join("\n"),
    };
  }

  return {
    ok: false,
    text: [
      `# not found: ${locator.raw}`,
      ...(resolution.suggestions.length > 0
        ? ["did you mean:", ...resolution.suggestions.map((node) => symbolRow(context, node))]
        : []),
    ].join("\n"),
  };
}

function rankCandidates(query: string, nodes: GraphNode[], requestedLabel: string | undefined): GraphNode[] {
  const wanted = query.trim().toLowerCase();
  const visible = requestedLabel === undefined ? nodes.filter((node) => !isContainerNode(node)) : nodes;
  const score = (node: GraphNode) =>
    (node.name.toLowerCase() === wanted ? 0 : 4) +
    (isTypeNode(node) ? 0 : 2) +
    (isTestPath(node.filePath) ? 1 : 0);
  return visible
    .map((node, index) => ({ node, index, score: score(node) }))
    .sort((left, right) => left.score - right.score || left.index - right.index)
    .map((entry) => entry.node);
}

function candidateNode(candidate: SymbolCandidate): GraphNode {
  const line = candidate.line ?? 0;
  return {
    qualifiedName: candidate.qualifiedName,
    name: (candidate.qualifiedName.split(".").pop() ?? "").replace(/#\d+$/, ""),
    label: candidate.label,
    filePath: candidate.filePath,
    startLine: line,
    endLine: line,
  };
}

function edgeNode(edge: TraceEdge): GraphNode {
  const line = edge.line ?? 0;
  return {
    qualifiedName: edge.relatedQualifiedName,
    name: edge.relatedQualifiedName.split(".").pop() ?? "",
    label: "",
    filePath: edge.filePath,
    startLine: line,
    endLine: line,
  };
}

async function searchRow(context: CodeContext, group: GraphNode[]): Promise<string> {
  const node = group[0];
  const lines = await context.reader.lines(node.filePath);
  const declaration =
    lines === undefined || node.startLine <= 0
      ? ""
      : declarationLine(lines, { ...node, endLine: node.startLine + 12 });
  const overloads = group.length > 1 ? `(${group.length} overloads)` : "";
  return [symbolRow(context, node), declaration, overloads].join("\t").trimEnd();
}

function baseName(node: GraphNode): string {
  return node.qualifiedName.replace(/#\d+$/, "");
}

function groupOverloads(nodes: GraphNode[]): GraphNode[][] {
  const groups = new Map<string, GraphNode[]>();
  for (const node of nodes) {
    const group = groups.get(baseName(node));
    if (group === undefined) groups.set(baseName(node), [node]);
    else group.push(node);
  }
  return [...groups.values()].map((group) =>
    group.sort((left, right) => left.startLine - right.startLine),
  );
}

interface FileRow {
  filePath: string;
  fields: string[];
}

/** Prints each file path once, followed by its indented rows. */
function groupByFile(rows: FileRow[]): string[] {
  const byFile = new Map<string, string[]>();
  for (const row of rows) {
    const line = `  ${row.fields.join("\t").trimEnd()}`;
    const existing = byFile.get(row.filePath);
    if (existing === undefined) byFile.set(row.filePath, [line]);
    else existing.push(line);
  }
  return [...byFile.entries()].flatMap(([filePath, lines]) => [filePath, ...lines]);
}

/**
 * A pattern is a regular expression, as in rg. One that is not valid, or that
 * matches nothing as one, is retried as literal text, so `findField(` and
 * `retry[0]` need no escaping.
 */
async function findText(
  context: CodeContext,
  pattern: string,
  regex: boolean,
  options: { pathFilter?: string; limit: number },
): Promise<{ matches: TextMatch[]; total: number; literal: boolean }> {
  if (regex && /[\\^$.|?*+()[\]{}]/.test(pattern) && isValidRegex(pattern)) {
    try {
      const found = await textSearch(context.client, pattern, { ...options, regex: true });
      if (found.matches.length > 0) return { ...found, literal: false };
    } catch {
      // The backend's regex dialect rejected it; fall through to literal text.
    }
  }
  return { ...(await textSearch(context.client, pattern, { ...options, regex: false })), literal: true };
}

/**
 * Matches in files the index does not hold: those created after its last run
 * and, when paths are given, those it does not cover (docs, scripts, a
 * directory left out of it). The backend reads indexed files from disk, so a
 * local scan is only needed for these.
 */
async function unindexedMatches(
  context: CodeContext,
  pattern: string,
  regex: boolean,
  pathFilter: string | undefined,
  scoped: boolean,
): Promise<TextMatch[]> {
  if (context.newFiles === undefined) return [];
  const candidates = new Set(await context.newFiles.list());
  if (scoped) {
    const indexed = await allFilePaths(context.client);
    // A truncated list cannot tell which files are missing from the index.
    if (indexed.length < FILE_LIST_MAX_ROWS) {
      const known = new Set(indexed);
      for (const path of await context.newFiles.all()) {
        if (!known.has(path)) candidates.add(path);
      }
    }
  }

  let filter: RegExp | undefined;
  try {
    filter = pathFilter === undefined ? undefined : new RegExp(pathFilter);
  } catch {
    filter = undefined;
  }
  const files = [...candidates]
    .filter((path) => !BINARY_FILE.test(path) && (filter === undefined || filter.test(path)))
    .slice(0, LOCAL_SCAN_MAX_FILES);

  const scan = async (matchesLine: (line: string) => boolean): Promise<TextMatch[]> => {
    const matches: TextMatch[] = [];
    for (const filePath of files) {
      const lines = await context.reader.lines(filePath);
      if (lines === undefined || lines.length > LOCAL_SCAN_MAX_LINES) continue;
      const hits = lines.flatMap((line, index) => (matchesLine(line) ? [index + 1] : []));
      if (hits.length > 0) matches.push({ enclosing: undefined, filePath, lines: hits });
    }
    return matches;
  };

  if (regex && isValidRegex(pattern)) {
    const expression = new RegExp(pattern);
    const found = await scan((line) => expression.test(line));
    if (found.length > 0) return found;
  }
  return scan((line) => line.includes(pattern));
}

function isValidRegex(pattern: string): boolean {
  try {
    new RegExp(pattern);
    return true;
  } catch {
    return false;
  }
}

/**
 * One regex for the backend path filter out of rg-style path arguments: a file
 * name, a relative file path, a directory, or a bare type name.
 */
export function pathsPattern(root: string, paths: string[]): string {
  const parts = paths.map((raw) => {
    const path = normalizePath(isAbsolute(raw) ? relative(root, raw) : raw).replace(/\/+$/, "");
    const escaped = escapeForRegex(path);
    // A path that exists is that one; `src` must not also mean `other/src`.
    const anchor = existsSync(join(root, path)) ? "^" : "(^|/)";
    return anchor + (/\.[A-Za-z0-9]+$/.test(path) ? `${escaped}$` : `${escaped}(/|\\.[^/]*$)`);
  });
  return `(${parts.join("|")})`;
}

/**
 * rg-style output: the file path once, then `line:text` for matches and
 * `line-text` for context, with `--` between separate windows.
 */
async function contextBlocks(
  context: CodeContext,
  matches: TextMatch[],
  around: number,
): Promise<{ lines: string[]; printed: Set<string>; skipped: number }> {
  const byFile = new Map<string, Set<number>>();
  for (const match of matches) {
    const lines = byFile.get(match.filePath) ?? new Set<number>();
    for (const line of match.lines) lines.add(line);
    byFile.set(match.filePath, lines);
  }

  const output: string[] = [];
  const printedFiles = new Set<string>();
  let printed = 0;
  let skipped = 0;
  for (const [filePath, matchLines] of byFile) {
    const fileLines = await context.reader.lines(filePath);
    if (fileLines === undefined) continue;
    const sorted = [...matchLines].sort((left, right) => left - right);
    if (printed >= CONTEXT_MAX_LINES) {
      skipped += sorted.length;
      continue;
    }

    output.push(filePath);
    printedFiles.add(filePath);
    let lastPrinted = 0;
    for (const [index, line] of sorted.entries()) {
      if (printed >= CONTEXT_MAX_LINES) {
        skipped += sorted.length - index;
        break;
      }
      const start = Math.max(lastPrinted + 1, line - around, 1);
      const end = Math.min(fileLines.length, line + around);
      if (lastPrinted > 0 && start > lastPrinted + 1) output.push("--");
      for (let current = start; current <= end; current += 1) {
        const text = fileLines[current - 1];
        const shown = text.length > CONTEXT_LINE_MAX_CHARS ? `${text.slice(0, CONTEXT_LINE_MAX_CHARS - 1)}…` : text;
        output.push(`${current}${matchLines.has(current) ? ":" : "-"}${shown}`);
        printed += 1;
      }
      lastPrinted = Math.max(lastPrinted, end);
    }
  }
  if (skipped > 0) {
    output.push(`# ... ${skipped} more matching lines not shown; give paths or lower -C`);
  }
  return { lines: output, printed: printedFiles, skipped };
}

function lineList(lines: number[]): string {
  const shown = lines.slice(0, TEXT_LINES_SHOWN).join(",");
  return lines.length > TEXT_LINES_SHOWN ? `${shown},+${lines.length - TEXT_LINES_SHOWN}` : shown;
}

/**
 * The line as it is in the file, indentation included: an edit is written from
 * this text, and in Python a guessed indent is a syntax error.
 */
async function lineText(context: CodeContext, filePath: string, line: number): Promise<string> {
  const lines = await context.reader.lines(filePath);
  const text = (lines?.[line - 1] ?? "").replace(/\s+$/, "");
  return text.length > TEXT_LINE_MAX_CHARS ? `${text.slice(0, TEXT_LINE_MAX_CHARS - 1)}…` : text;
}

/** Declarations of constants first, then production code, then tests. */
function sortMatches(matches: TextMatch[]): TextMatch[] {
  const score = (match: TextMatch) =>
    (isTestPath(match.filePath) ? 2 : 0) + (match.enclosing?.label === "Field" ? 0 : 1);
  return matches
    .map((match, index) => ({ match, index, score: score(match) }))
    .sort((left, right) => left.score - right.score || left.index - right.index)
    .map((entry) => entry.match);
}

function capLines(block: string): string {
  const lines = block.split("\n");
  if (lines.length <= SNIPPET_MAX_LINES + 1) return block;
  return [...lines.slice(0, SNIPPET_MAX_LINES + 1), `# ... ${lines.length - SNIPPET_MAX_LINES - 1} more lines; gcal get for the rest`].join("\n");
}

function escapeForRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
