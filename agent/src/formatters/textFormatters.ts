import type {
  HydratedSymbolCandidate,
  MultiHopWorkflowResult,
  SelectedSymbol,
  SnippetManifest,
  SymbolCandidate,
  TraceEdge,
  TraceHint,
} from "../domain/types.js";

function fieldValue(value: string | number | null): string {
  return value === null ? "" : String(value);
}

function locationValue(filePath: string, line: number | null): string {
  const renderedLine = fieldValue(line);
  return renderedLine.length > 0 ? `${filePath}:${renderedLine}` : filePath;
}

function formatTraceEdge(edge: TraceEdge): string {
  return [
    edge.relatedQualifiedName,
    fieldValue(edge.hop),
    edge.filePath,
    fieldValue(edge.line),
  ].join("\t");
}

export function formatCandidatesText(candidates: SymbolCandidate[]): string {
  return candidates
    .map((candidate) =>
      [
        candidate.qualifiedName,
        candidate.label,
        candidate.filePath,
        fieldValue(candidate.line),
        candidate.signature,
      ].join("\t"),
    )
    .join("\n");
}

export function formatCandidateBlockText(candidates: SymbolCandidate[]): string {
  return [
    "# candidates",
    ...candidates.map((candidate, index) =>
      [
        String(index + 1),
        candidate.label,
        candidate.qualifiedName,
        locationValue(candidate.filePath, candidate.line),
        candidate.signature,
      ].join("\t"),
    ),
  ].join("\n");
}

export function formatSelectedMetadataText(
  selected: SelectedSymbol,
  warnings: string[] = [],
): string {
  return [
    "# selected",
    `qualified_name=${selected.qualifiedName}`,
    `kind=${selected.kind}`,
    `file=${selected.filePath}`,
    `line=${fieldValue(selected.startLine)}`,
    `end_line=${fieldValue(selected.endLine)}`,
    `lines=${fieldValue(selected.lines)}`,
    `complexity=${fieldValue(selected.complexity)}`,
    `cognitive=${fieldValue(selected.cognitive)}`,
    `visibility=${selected.visibility}`,
    `signature=${selected.signature}`,
    `return_type=${selected.returnType}`,
    `decorators=${selected.decorators}`,
    `callers=${fieldValue(selected.callers)}`,
    `callees=${fieldValue(selected.callees)}`,
    `warnings=${warnings.join(",")}`,
  ].join("\n");
}

export function formatTraceSectionText(name: string, trace: TraceEdge[] | TraceHint): string {
  if (Array.isArray(trace)) {
    return [`# ${name}`, ...trace.map(formatTraceEdge)].join("\n");
  }

  return [`# ${name}`, `hint\t${trace.count}\t${trace.command}`].join("\n");
}

export function formatTraceRowsText(trace: TraceEdge[]): string {
  return trace.map(formatTraceEdge).join("\n");
}

export function formatSourceText(selected: SelectedSymbol): string {
  return formatBoundedSourceText(selected);
}

export const WORKFLOW_MAX_SOURCE_BYTES = 12 * 1024;

export function formatMultiHopWorkflowText(result: MultiHopWorkflowResult): string {
  const sections = [
    result.candidates.length > 0 ? formatCandidateBlockText(result.candidates) : "",
    ["# selected", `qualified_name=${result.selectedQualifiedName}`].join("\n"),
    result.source === undefined
      ? ""
      : ["# source", formatBoundedSourceText(result.source, WORKFLOW_MAX_SOURCE_BYTES)].join("\n"),
    result.inbound === undefined ? "" : formatTraceSectionText("inbound", result.inbound),
    result.outbound === undefined ? "" : formatTraceSectionText("outbound", result.outbound),
  ].filter((section) => section.length > 0);

  return sections.join("\n\n");
}

export function formatSnippetManifestText(manifest: SnippetManifest): string {
  const qualifiedName = manifest.selected.qualifiedName;
  return [
    `snippet-too-large\t${qualifiedName}`,
    `bytes\t${manifest.sourceBytes}`,
    `lines\t${manifest.sourceLines}`,
    `chunks\t${manifest.chunkCount}`,
    `chunk-bytes\t${manifest.chunkBytes}`,
    `source-sha256\t${manifest.sourceSha256}`,
    `next\tack get ${qualifiedName} --chunk 1 --expected-source-sha ${manifest.sourceSha256}`,
  ].join("\n");
}

export const BATCH_GET_MAX_OUTPUT_BYTES = 48 * 1024;
export const BATCH_GET_MAX_SOURCE_BYTES = 12 * 1024;
export const HYDRATED_SEARCH_MAX_OUTPUT_BYTES = 24 * 1024;
export const HYDRATED_SEARCH_MAX_SNIPPET_BYTES = 4 * 1024;

const BATCH_GET_MAX_HEADER_ID_BYTES = 512;
const HYDRATED_SEARCH_MAX_CANDIDATE_BYTES = 8 * 1024;
const BATCH_SOURCE_TRUNCATION_MARKER =
  "\n... [truncated; run single-symbol gcal get for full source]";
const SEARCH_CANDIDATE_TRUNCATION_MARKER = "\n... [candidate rows truncated]";
const SEARCH_SNIPPET_TRUNCATION_MARKER = "\n... [snippet truncated; run gcal get for full source]";

export function formatBatchSourcesText(selectedSymbols: SelectedSymbol[]): string {
  if (selectedSymbols.length === 0) {
    return "";
  }

  const separator = "\n\n";
  const separatorBytes = Buffer.byteLength(separator, "utf8");
  const availableBlockBytes =
    BATCH_GET_MAX_OUTPUT_BYTES - separatorBytes * (selectedSymbols.length - 1);
  const fairBlockBytes = Math.floor(availableBlockBytes / selectedSymbols.length);

  return selectedSymbols
    .map((selected) => {
      const header = `# ${truncateHeaderId(selected.qualifiedName)}\n`;
      const sourceBytes = Math.max(
        0,
        Math.min(BATCH_GET_MAX_SOURCE_BYTES, fairBlockBytes - Buffer.byteLength(header, "utf8")),
      );
      return `${header}${formatBoundedSourceText(
        selected,
        sourceBytes,
        BATCH_SOURCE_TRUNCATION_MARKER,
      )}`;
    })
    .join(separator);
}

export function formatHydratedSearchText(
  candidates: SymbolCandidate[],
  snippets: HydratedSymbolCandidate[],
): string {
  const candidateText = truncateUtf8(
    formatCandidatesText(candidates),
    HYDRATED_SEARCH_MAX_CANDIDATE_BYTES,
    SEARCH_CANDIDATE_TRUNCATION_MARKER,
  );
  const sectionCount = (candidateText.length > 0 ? 1 : 0) + snippets.length;
  const separator = "\n\n";
  const separatorBytes = Buffer.byteLength(separator, "utf8");
  const separatorsBytes = Math.max(0, sectionCount - 1) * separatorBytes;
  const candidateBytes = Buffer.byteLength(candidateText, "utf8");
  const availableSnippetBytes = HYDRATED_SEARCH_MAX_OUTPUT_BYTES - candidateBytes - separatorsBytes;
  const fairSnippetBytes =
    snippets.length === 0 ? 0 : Math.floor(availableSnippetBytes / snippets.length);
  const snippetBlocks = snippets.map(({ candidate, selected }) => {
    const header = `# snippet\t${truncateHeaderId(candidate.qualifiedName)}\n`;
    const sourceBytes = Math.max(
      0,
      Math.min(
        HYDRATED_SEARCH_MAX_SNIPPET_BYTES,
        fairSnippetBytes - Buffer.byteLength(header, "utf8"),
      ),
    );
    return `${header}${formatBoundedSourceText(
      selected,
      sourceBytes,
      SEARCH_SNIPPET_TRUNCATION_MARKER,
    )}`;
  });

  return [candidateText, ...snippetBlocks].filter((section) => section.length > 0).join(separator);
}

function truncateHeaderId(qualifiedName: string): string {
  if (Buffer.byteLength(qualifiedName, "utf8") <= BATCH_GET_MAX_HEADER_ID_BYTES) {
    return qualifiedName;
  }

  const marker = "...";
  return `${utf8Prefix(
    qualifiedName,
    BATCH_GET_MAX_HEADER_ID_BYTES - Buffer.byteLength(marker, "utf8"),
  )}${marker}`;
}

function formatBoundedSourceText(
  selected: SelectedSymbol,
  maxBytes?: number,
  truncationMarker = BATCH_SOURCE_TRUNCATION_MARKER,
): string {
  const chunk = selected.sourceChunk;
  const outline =
    chunk === undefined || chunk.chunkCount <= 1
      ? ""
      : [
          `... [chunk ${chunk.chunk}/${chunk.chunkCount}; ${chunk.sourceBytes} bytes; ${chunk.sourceLines} lines; sha256 ${chunk.sourceSha256}]`,
          chunk.eof
            ? "... [end of snippet]"
            : `... [next: gcal get ${selected.qualifiedName} --chunk ${chunk.chunk + 1} --expected-source-sha ${chunk.sourceSha256}]`,
        ].join("\n");

  if (maxBytes === undefined) {
    return outline.length === 0 ? selected.source : `${selected.source}\n${outline}`;
  }

  if (outline.length === 0) {
    return truncateUtf8(selected.source, maxBytes, truncationMarker);
  }

  const separator = "\n";
  const outlineBytes = Buffer.byteLength(outline, "utf8");
  const separatorBytes = Buffer.byteLength(separator, "utf8");
  if (outlineBytes + separatorBytes >= maxBytes) {
    return utf8Prefix(outline, maxBytes);
  }

  const sourceBudget = maxBytes - outlineBytes - separatorBytes;
  return `${utf8Prefix(selected.source, sourceBudget)}${separator}${outline}`;
}

function truncateUtf8(
  value: string,
  maxBytes: number,
  marker = BATCH_SOURCE_TRUNCATION_MARKER,
): string {
  if (Buffer.byteLength(value, "utf8") <= maxBytes) {
    return value;
  }

  const markerBytes = Buffer.byteLength(marker, "utf8");
  if (maxBytes <= markerBytes) {
    return utf8Prefix(marker, maxBytes);
  }

  return `${utf8Prefix(value, maxBytes - markerBytes)}${marker}`;
}

function utf8Prefix(value: string, maxBytes: number): string {
  let bytes = 0;
  let prefix = "";

  for (const character of value) {
    const characterBytes = Buffer.byteLength(character, "utf8");
    if (bytes + characterBytes > maxBytes) {
      break;
    }

    prefix += character;
    bytes += characterBytes;
  }

  return prefix;
}
