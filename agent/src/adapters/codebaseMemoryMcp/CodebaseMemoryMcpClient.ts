import type {
  SearchOptions,
  SnippetChunkOptions,
  TraceEdge,
  TraceOptions,
} from "../../domain/types.js";
import type { GcalBackendClient } from "../../domain/GcalBackendClient.js";
import {
  compileLiteralSearch,
  isFtsSyntaxError,
  mergeLiteralSearchResults,
  type LiteralSearchBranch,
} from "./literalSearch.js";
import {
  normalizeArchitectureResponse,
  normalizeProjectsResponse,
  normalizeSearchResponse,
  normalizeSelectedSymbol,
  normalizeSnippetChunk,
  normalizeSnippetManifest,
} from "./normalize.js";

export interface McpToolInvoker {
  invoke(toolName: string, args: Record<string, unknown>): Promise<unknown>;
}

type TraceDirection = "inbound" | "outbound";

export class CodebaseMemoryMcpClient implements GcalBackendClient {
  constructor(
    private readonly project: string,
    private readonly invoker: McpToolInvoker,
  ) {}

  async search(query: string, options: Partial<SearchOptions>) {
    const limit = options.limit ?? 20;
    if (limit === 0) return [];

    const branchResults = [];
    for (const branch of compileLiteralSearch(query)) {
      branchResults.push(await this.searchBranch(branch, options, limit));
    }

    return mergeLiteralSearchResults(branchResults, limit);
  }

  private async searchBranch(
    branch: LiteralSearchBranch,
    options: Partial<SearchOptions>,
    limit: number,
  ) {
    const branchArgument =
      branch.kind === "query" ? { query: branch.value } : { name_pattern: branch.value };

    let raw: unknown;
    try {
      raw = await this.invoke("search_graph", {
        project: this.project,
        ...branchArgument,
        limit,
        label: options.label,
        file_pattern: options.filePattern,
        qn_pattern: options.qualifiedNamePattern,
      });
    } catch (error) {
      if (isFtsSyntaxError(error)) {
        throw new Error("GCAL search backend rejected a literal-safe query");
      }
      throw error;
    }

    return normalizeSearchResponse(raw);
  }

  async symbol(nameRegex: string, options: Partial<SearchOptions>) {
    const raw = await this.invoke("search_graph", {
      project: this.project,
      name_pattern: nameRegex,
      limit: options.limit ?? 20,
      label: options.label,
      file_pattern: options.filePattern,
      qn_pattern: options.qualifiedNamePattern,
    });

    return normalizeSearchResponse(raw);
  }

  async get(qualifiedName: string) {
    const raw = await this.invoke("get_code_snippet", {
      project: this.project,
      qualified_name: qualifiedName,
    });

    return normalizeSelectedSymbol(raw);
  }

  async getSnippetManifest(qualifiedName: string, chunkBytes: number) {
    const raw = await this.invoke("get_code_snippet_manifest", {
      project: this.project,
      qualified_name: qualifiedName,
      chunk_bytes: chunkBytes,
    });

    return normalizeSnippetManifest(raw);
  }

  async getSnippetChunk(qualifiedName: string, options: SnippetChunkOptions) {
    const raw = await this.invoke("get_code_snippet_chunk", {
      project: this.project,
      qualified_name: qualifiedName,
      chunk: options.chunk,
      chunk_bytes: options.chunkBytes,
      expected_source_sha256: options.expectedSourceSha256,
    });

    return normalizeSnippetChunk(raw);
  }

  async callers(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.trace(qualifiedName, "inbound", options.depth);
  }

  async callees(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]> {
    return this.trace(qualifiedName, "outbound", options.depth);
  }

  async arch(): Promise<unknown> {
    const raw = await this.invoke("get_architecture", {
      project: this.project,
      aspects: [
        "languages",
        "packages",
        "entry_points",
        "hotspots",
        "boundaries",
        "layers",
        "clusters",
      ],
    });
    return normalizeArchitectureResponse(raw);
  }

  async status(): Promise<unknown> {
    return this.invoke("index_status", { project: this.project });
  }

  async index(repoPath: string): Promise<unknown> {
    return this.invoke("index_repository", { repo_path: repoPath });
  }

  async projects() {
    return normalizeProjectsResponse(await this.invoke("list_projects", {}));
  }

  private async trace(
    qualifiedName: string,
    direction: TraceDirection,
    depth: number,
  ): Promise<TraceEdge[]> {
    const raw = await this.invoke("trace_path", {
      project: this.project,
      function_name: qualifiedName,
      direction,
      depth,
      mode: "calls",
    });

    return traceRows(raw, direction).map((row) => traceEdge(row, qualifiedName, direction));
  }

  private invoke(toolName: string, args: Record<string, unknown>): Promise<unknown> {
    return this.invoker.invoke(toolName, removeUndefined(args));
  }
}

function removeUndefined(args: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(Object.entries(args).filter(([, value]) => value !== undefined));
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function traceRows(raw: unknown, direction: TraceDirection): Array<Record<string, unknown>> {
  if (!isRecord(raw)) return [];
  if (Array.isArray(raw.paths)) return raw.paths.filter(isRecord);

  const rows = direction === "inbound" ? raw.callers : raw.callees;
  return Array.isArray(rows) ? rows.filter(isRecord) : [];
}

function firstString(...values: unknown[]): string {
  for (const value of values) {
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }

  return "";
}

function firstNumber(...values: unknown[]): number | null {
  for (const value of values) {
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }

    if (typeof value === "string" && value.trim() !== "") {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) {
        return parsed;
      }
    }
  }

  return null;
}

function traceEdge(
  row: Record<string, unknown>,
  qualifiedName: string,
  direction: TraceDirection,
): TraceEdge {
  const currentRelated = firstString(row.qualified_name, row.qn, row.name);
  const sourceQualifiedName = firstString(
    row.sourceQualifiedName,
    row.source_qualified_name,
    row.source,
    row.caller,
    direction === "inbound" ? currentRelated : qualifiedName,
  );
  const targetQualifiedName = firstString(
    row.targetQualifiedName,
    row.target_qualified_name,
    row.target,
    row.callee,
    direction === "outbound" ? currentRelated : qualifiedName,
  );

  return {
    sourceQualifiedName,
    targetQualifiedName,
    relatedQualifiedName: direction === "inbound" ? sourceQualifiedName : targetQualifiedName,
    hop: firstNumber(row.hop),
    filePath: firstString(row.filePath, row.file_path, row.file, row.path),
    line: firstNumber(row.line, row.start_line),
  };
}
