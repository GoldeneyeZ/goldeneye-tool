// Typed read access to the backend code graph. Every function is one backend
// round trip and returns plain rows; presentation lives in the formatters.

export interface ToolClient {
  tool(toolName: string, args: Record<string, unknown>): Promise<unknown>;
}

export interface GraphNode {
  qualifiedName: string;
  name: string;
  label: string;
  filePath: string;
  startLine: number;
  endLine: number;
}

export interface TextMatch {
  /** Innermost indexed symbol containing the matched lines, when there is one. */
  enclosing: GraphNode | undefined;
  filePath: string;
  lines: number[];
}

export interface TextSearchOptions {
  regex?: boolean;
  pathFilter?: string;
  limit?: number;
}

export interface Subtype {
  node: GraphNode;
  relation: string;
}

const TYPE_LABELS = new Set([
  "Class",
  "Interface",
  "Enum",
  "Struct",
  "Trait",
  "Type",
  "Record",
  "Union",
  "Protocol",
  "Object",
  "Annotation",
]);
const CONTAINER_LABELS = new Set(["File", "Module", "Project", "Folder", "Package"]);

export function isTypeNode(node: GraphNode): boolean {
  return TYPE_LABELS.has(node.label);
}

export function isContainerNode(node: GraphNode): boolean {
  return CONTAINER_LABELS.has(node.label);
}

export function escapeRegex(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Finds symbols whose qualified name ends with the given segments in order.
 * Extra segments may sit between them, so both `Class.method` and a natural
 * package-qualified name resolve against file-scoped backend identifiers.
 */
export async function findBySegments(
  client: ToolClient,
  segments: string[],
  overload: number | undefined,
  limit = 50,
): Promise<GraphNode[]> {
  const last = segments[segments.length - 1];
  const chain = segments.map(escapeRegex).join("(\\.[^.]+)*\\.");
  const suffix = overload === undefined ? "(#\\d+)?" : overload <= 1 ? "" : `#${overload}`;
  const raw = await client.tool("search_graph", {
    name_pattern: `^${escapeRegex(last)}$`,
    qn_pattern: `(^|\\.)${chain}${suffix}$`,
    limit,
  });
  return searchRows(raw);
}

export async function findByExactName(client: ToolClient, qualifiedName: string): Promise<GraphNode[]> {
  const raw = await client.tool("search_graph", {
    qn_pattern: `^${escapeRegex(qualifiedName)}$`,
    limit: 5,
  });
  return searchRows(raw);
}

export async function findByName(client: ToolClient, name: string, limit = 8): Promise<GraphNode[]> {
  const raw = await client.tool("search_graph", {
    name_pattern: `^${escapeRegex(name)}$`,
    limit,
  });
  return searchRows(raw);
}

/**
 * Regex matching names that differ from `name` by up to three extra characters
 * at each word boundary, or by a plural `s` on a word.
 */
export function similarNamePattern(name: string): string {
  const words = name.match(/[A-Z]+(?![a-z])|[A-Z]?[a-z0-9]+|_+/g) ?? [name];
  const loose = words.map((word) =>
    word.length > 3 && /s$/i.test(word) ? `${escapeRegex(word.slice(0, -1))}s?` : escapeRegex(word),
  );
  return `(?i)^${loose.join(".{0,3}")}.{0,3}$`;
}

export async function findBySimilarName(client: ToolClient, name: string, limit = 5): Promise<GraphNode[]> {
  if (name.length < 4) return [];
  const raw = await client.tool("search_graph", { name_pattern: similarNamePattern(name), limit });
  return searchRows(raw);
}

export async function nodesInFile(client: ToolClient, filePath: string): Promise<GraphNode[]> {
  return queryNodes(
    client,
    `MATCH (n) WHERE n.file_path = ${cypherString(filePath)} ` +
      `RETURN n.qualified_name, n.name, labels(n), n.file_path, n.start_line, n.end_line ` +
      `ORDER BY n.start_line`,
    2_000,
  );
}

export async function nodesAtLine(
  client: ToolClient,
  filePath: string,
  line: number,
): Promise<GraphNode[]> {
  const nodes = await queryNodes(
    client,
    `MATCH (n) WHERE n.file_path = ${cypherString(filePath)} ` +
      `AND n.start_line <= ${line} AND n.end_line >= ${line} ` +
      `RETURN n.qualified_name, n.name, labels(n), n.file_path, n.start_line, n.end_line`,
    200,
  );
  // Innermost first.
  return nodes.sort(
    (left, right) => left.endLine - left.startLine - (right.endLine - right.startLine),
  );
}

export async function filesBySuffix(client: ToolClient, pathSuffix: string): Promise<string[]> {
  const raw = await client.tool("query_graph", {
    query:
      `MATCH (f:File) WHERE f.file_path ENDS WITH ${cypherString(pathSuffix)} ` +
      `RETURN f.file_path LIMIT 20`,
    max_rows: 20,
  });
  return queryRows(raw)
    .map((row) => (typeof row[0] === "string" ? row[0] : ""))
    .filter((path) => path === pathSuffix || path.endsWith(`/${pathSuffix}`));
}

export async function membersOf(client: ToolClient, qualifiedName: string): Promise<GraphNode[]> {
  return queryNodes(
    client,
    `MATCH (c)-[:DEFINES]->(m) WHERE c.qualified_name = ${cypherString(qualifiedName)} ` +
      `RETURN m.qualified_name, m.name, labels(m), m.file_path, m.start_line, m.end_line ` +
      `ORDER BY m.start_line`,
    1_000,
  );
}

export async function subtypesOf(client: ToolClient, qualifiedName: string): Promise<Subtype[]> {
  const raw = await client.tool("query_graph", {
    query:
      `MATCH (s)-[r:INHERITS|IMPLEMENTS]->(t) WHERE t.qualified_name = ${cypherString(qualifiedName)} ` +
      `RETURN s.qualified_name, s.name, labels(s), s.file_path, s.start_line, s.end_line, type(r)`,
    max_rows: 500,
  });
  return queryRows(raw).map((row) => ({
    node: nodeFromRow(row),
    relation: typeof row[6] === "string" ? row[6] : "",
  }));
}

/** The backend caps one query at 10000 rows; larger projects get a partial layout. */
export const FILE_LIST_MAX_ROWS = 10_000;

export async function allFilePaths(client: ToolClient, maxRows = FILE_LIST_MAX_ROWS): Promise<string[]> {
  const raw = await client.tool("query_graph", {
    query: "MATCH (f:File) RETURN f.file_path",
    max_rows: maxRows,
  });
  return queryRows(raw)
    .map((row) => row[0])
    .filter((path): path is string => typeof path === "string" && path.length > 0);
}

export async function textSearch(
  client: ToolClient,
  pattern: string,
  options: TextSearchOptions = {},
): Promise<{ matches: TextMatch[]; total: number }> {
  const raw = await client.tool("search_code", {
    pattern,
    regex: options.regex ?? false,
    path_filter: options.pathFilter,
    limit: options.limit ?? 30,
    mode: "compact",
  });
  if (!isRecord(raw)) return { matches: [], total: 0 };

  const matches: TextMatch[] = [];
  for (const row of arrayOf(raw.results)) {
    if (!isRecord(row)) continue;
    const filePath = stringOf(row.file) || stringOf(row.file_path);
    const lines = arrayOf(row.match_lines).filter((line): line is number => typeof line === "number");
    if (filePath.length === 0 || lines.length === 0) continue;
    const label = stringOf(row.label);
    matches.push({
      filePath,
      lines,
      enclosing:
        label.length === 0 || CONTAINER_LABELS.has(label)
          ? undefined
          : {
              qualifiedName: stringOf(row.qualified_name),
              name: stringOf(row.node),
              label,
              filePath,
              startLine: numberOf(row.start_line),
              endLine: numberOf(row.end_line),
            },
    });
  }
  for (const row of arrayOf(raw.raw_matches)) {
    if (!isRecord(row)) continue;
    const filePath = stringOf(row.file) || stringOf(row.file_path);
    const line = numberOf(row.line);
    if (filePath.length > 0 && line > 0) matches.push({ filePath, lines: [line], enclosing: undefined });
  }

  // total_results counts matching symbols before the limit; total_grep_matches counts lines.
  const total = numberOf(raw.total_results) || matches.length;
  return { matches, total };
}

export async function projectRoot(client: ToolClient): Promise<string> {
  const raw = await client.tool("index_status", {});
  const root = isRecord(raw) ? stringOf(raw.root_path) : "";
  if (root.length === 0) throw new Error("backend did not report a project root path");
  return root;
}

async function queryNodes(client: ToolClient, query: string, maxRows: number): Promise<GraphNode[]> {
  return queryRows(await client.tool("query_graph", { query, max_rows: maxRows })).map(nodeFromRow);
}

function queryRows(raw: unknown): unknown[][] {
  if (!isRecord(raw)) return [];
  return arrayOf(raw.rows).filter((row): row is unknown[] => Array.isArray(row));
}

function nodeFromRow(row: unknown[]): GraphNode {
  const labels = Array.isArray(row[2]) ? row[2] : [row[2]];
  return {
    qualifiedName: stringOf(row[0]),
    name: stringOf(row[1]),
    label: stringOf(labels[0]),
    filePath: stringOf(row[3]),
    startLine: numberOf(row[4]),
    endLine: numberOf(row[5]),
  };
}

function searchRows(raw: unknown): GraphNode[] {
  if (!isRecord(raw)) return [];
  return arrayOf(raw.results)
    .filter(isRecord)
    .map((row) => ({
      qualifiedName: stringOf(row.qualified_name),
      name: stringOf(row.name),
      label: stringOf(row.label),
      filePath: stringOf(row.file_path),
      startLine: numberOf(row.start_line),
      endLine: numberOf(row.end_line),
    }))
    .filter((node) => node.qualifiedName.length > 0);
}

function cypherString(value: string): string {
  if (/['\\\r\n]/.test(value)) {
    throw new Error(`unsupported characters in graph lookup value: ${value}`);
  }
  return `'${value}'`;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function arrayOf(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function stringOf(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function numberOf(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}
