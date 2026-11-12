import type {
  IndexedProject,
  SelectedSymbol,
  SymbolCandidate,
  SymbolKind,
  SymbolLabel,
} from "../../domain/types.js";
import {
  rawArchitectureResponseSchema,
  rawProjectsResponseSchema,
  rawSearchResponseSchema,
  rawSnippetChunkSchema,
  rawSnippetManifestSchema,
  rawSnippetSchema,
} from "./mcpSchemas.js";
import type { SnippetManifest } from "../../domain/types.js";

const architectureScalarKeys = ["project", "total_nodes", "total_edges"] as const;
const architectureSectionKeys = [
  "languages",
  "packages",
  "entry_points",
  "hotspots",
  "boundaries",
  "layers",
  "clusters",
] as const;
const architectureSectionLimit = 20;

function firstString(...values: Array<unknown>): string {
  for (const value of values) {
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return "";
}

function firstNumber(...values: Array<unknown>): number | null {
  for (const value of values) {
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return null;
}

export function toSymbolKind(rawKind: string | undefined): SymbolKind {
  switch (rawKind?.trim().toLowerCase()) {
    case "class":
      return "Class";
    case "method":
      return "Method";
    case "function":
    case "func":
      return "Function";
    case "field":
    case "property":
    case "member":
      return "Field";
    default:
      return "Unknown";
  }
}

function normalizeLabel(
  label: string | undefined,
  type: string | undefined,
  labels?: string[],
): SymbolLabel {
  const candidates = [label, type, ...(labels ?? [])];

  for (const candidate of candidates) {
    const kind = toSymbolKind(candidate);
    if (kind !== "Unknown") {
      return kind;
    }
  }

  return firstString(...candidates) || "Unknown";
}

export function normalizeSearchResponse(raw: unknown): SymbolCandidate[] {
  const parsed = rawSearchResponseSchema.parse(raw);
  const rows = Array.isArray(parsed)
    ? parsed
    : [...(parsed.results ?? []), ...(parsed.semantic_results ?? []), ...(parsed.matches ?? [])];

  return rows.map((row) => ({
    qualifiedName: firstString(row.qualified_name, row.qn, row.name),
    label: normalizeLabel(row.label, row.type, row.labels),
    filePath: firstString(row.file_path, row.file, row.path),
    line: firstNumber(row.start_line, row.line),
    signature: firstString(row.signature),
  }));
}

export function normalizeArchitectureResponse(raw: unknown): Record<string, unknown> {
  const parsed = rawArchitectureResponseSchema.parse(raw);
  const normalized: Record<string, unknown> = {};

  for (const key of architectureScalarKeys) {
    if (parsed[key] !== undefined) normalized[key] = parsed[key];
  }
  for (const key of architectureSectionKeys) {
    const value = parsed[key];
    if (Array.isArray(value)) normalized[key] = value.slice(0, architectureSectionLimit);
  }

  return normalized;
}

export function normalizeProjectsResponse(raw: unknown): IndexedProject[] {
  const parsed = rawProjectsResponseSchema.parse(raw);
  const projects = Array.isArray(parsed) ? parsed : parsed.projects;

  return projects
    .map((project) => ({
      name: firstString(project.name, project.project, project.project_id, project.id),
      rootPath: firstString(project.root_path, project.rootPath, project.path),
    }))
    .filter((project) => project.name.length > 0 && project.rootPath.length > 0);
}

export function normalizeSelectedSymbol(raw: unknown): SelectedSymbol {
  const parsed = rawSnippetSchema.parse(raw);

  return {
    qualifiedName: firstString(parsed.qualified_name, parsed.qn, parsed.name),
    kind: normalizeLabel(parsed.label, parsed.type, parsed.labels),
    filePath: firstString(parsed.file_path, parsed.file, parsed.path),
    startLine: firstNumber(parsed.start_line, parsed.line),
    endLine: firstNumber(parsed.end_line),
    lines: firstNumber(parsed.lines),
    complexity: firstNumber(parsed.complexity),
    cognitive: firstNumber(parsed.cognitive),
    visibility: firstString(parsed.visibility),
    signature: firstString(parsed.signature),
    returnType: firstString(parsed.return_type),
    decorators: firstString(parsed.decorators),
    callers: firstNumber(parsed.callers),
    callees: firstNumber(parsed.callees),
    source: firstString(parsed.code, parsed.source, parsed.snippet, parsed.content, parsed.text),
  };
}

export function normalizeSnippetManifest(raw: unknown): SnippetManifest {
  const parsed = rawSnippetManifestSchema.parse(raw);
  return {
    selected: normalizeSelectedSymbol(parsed),
    sourceBytes: parsed.source_bytes,
    sourceLines: parsed.source_lines,
    sourceSha256: parsed.source_sha256,
    indexedFileHash: parsed.indexed_file_hash,
    chunkBytes: parsed.chunk_bytes,
    chunkCount: parsed.chunk_count,
  };
}

export function normalizeSnippetChunk(raw: unknown): SelectedSymbol {
  const parsed = rawSnippetChunkSchema.parse(raw);
  return {
    ...normalizeSelectedSymbol(parsed),
    source: parsed.source,
    sourceChunk: {
      chunk: parsed.chunk,
      chunkCount: parsed.chunk_count,
      chunkBytes: parsed.chunk_bytes,
      sourceBytes: parsed.source_bytes,
      sourceLines: parsed.source_lines,
      sourceSha256: parsed.source_sha256,
      indexedFileHash: parsed.indexed_file_hash,
      chunkStartByte: parsed.chunk_start_byte,
      chunkEndByte: parsed.chunk_end_byte,
      eof: parsed.eof,
      truncated: parsed.truncated,
    },
  };
}
