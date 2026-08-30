export type SymbolKind = "Class" | "Method" | "Function" | "Field" | "Unknown";
export type SymbolLabel = SymbolKind | (string & Record<never, never>);

export interface SymbolCandidate {
  qualifiedName: string;
  label: SymbolLabel;
  filePath: string;
  line: number | null;
  signature: string;
}

export interface SelectedSymbol {
  qualifiedName: string;
  kind: SymbolLabel;
  filePath: string;
  startLine: number | null;
  endLine: number | null;
  lines: number | null;
  complexity: number | null;
  cognitive: number | null;
  visibility: string;
  signature: string;
  returnType: string;
  decorators: string;
  callers: number | null;
  callees: number | null;
  source: string;
  sourceChunk?: SourceChunkMetadata;
}

export interface SourceChunkMetadata {
  chunk: number;
  chunkCount: number;
  chunkBytes: number;
  sourceBytes: number;
  sourceLines: number;
  sourceSha256: string;
  indexedFileHash: string;
  chunkStartByte: number;
  chunkEndByte: number;
  eof: boolean;
  truncated: boolean;
}

export interface SnippetManifest {
  selected: SelectedSymbol;
  sourceBytes: number;
  sourceLines: number;
  sourceSha256: string;
  indexedFileHash: string;
  chunkBytes: number;
  chunkCount: number;
}

export interface SnippetChunkOptions {
  chunk: number;
  chunkBytes: number;
  expectedSourceSha256?: string;
}

export interface HydratedSymbolCandidate {
  candidate: SymbolCandidate;
  selected: SelectedSymbol;
}

export interface TraceEdge {
  sourceQualifiedName: string;
  targetQualifiedName: string;
  relatedQualifiedName: string;
  hop: number | null;
  filePath: string;
  line: number | null;
}

export interface InspectResult {
  candidates: SymbolCandidate[];
  selected: SelectedSymbol;
  inbound: TraceEdge[] | TraceHint;
  outbound: TraceEdge[] | TraceHint;
  warnings: string[];
}

export interface TraceHint {
  kind: "hint";
  count: number;
  command: string;
}

export interface SearchOptions {
  limit: number;
  label?: string;
  filePattern?: string;
  qualifiedNamePattern?: string;
}

export interface InspectOptions {
  limit: number;
}

export interface TraceOptions {
  depth: number;
}

export interface IndexedProject {
  name: string;
  rootPath: string;
}

export type WorkflowHop = "source" | "callers" | "callees";

export interface MultiHopWorkflowOptions {
  exact: boolean;
  rank: number;
  searchLimit: number;
  source: boolean;
  callers: boolean;
  callees: boolean;
  depth: number;
  traceLimit: number;
}

export interface WorkflowFailure {
  hop: WorkflowHop;
  message: string;
}

export interface MultiHopWorkflowResult {
  candidates: SymbolCandidate[];
  selectedQualifiedName: string;
  source?: SelectedSymbol;
  inbound?: TraceEdge[];
  inboundTotal?: number;
  outbound?: TraceEdge[];
  outboundTotal?: number;
  failures: WorkflowFailure[];
}
