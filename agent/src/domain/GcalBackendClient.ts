import type {
  IndexedProject,
  SearchOptions,
  SelectedSymbol,
  SnippetChunkOptions,
  SnippetManifest,
  SymbolCandidate,
  TraceEdge,
  TraceOptions,
} from "./types.js";

export class GcalBackendError extends Error {
  constructor(
    message: string,
    readonly code: string | undefined,
    readonly details: unknown,
  ) {
    super(message);
    this.name = "GcalBackendError";
  }
}

export interface GcalBackendClient {
  search(query: string, options: Partial<SearchOptions>): Promise<SymbolCandidate[]>;
  symbol(nameRegex: string, options: Partial<SearchOptions>): Promise<SymbolCandidate[]>;
  get(qualifiedName: string): Promise<SelectedSymbol>;
  getSnippetManifest?(
    qualifiedName: string,
    chunkBytes: number,
  ): Promise<SnippetManifest>;
  getSnippetChunk?(
    qualifiedName: string,
    options: SnippetChunkOptions,
  ): Promise<SelectedSymbol>;
  callers(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]>;
  callees(qualifiedName: string, options: TraceOptions): Promise<TraceEdge[]>;
  arch(): Promise<unknown>;
  status(): Promise<unknown>;
  index(repoPath: string): Promise<unknown>;
  projects(): Promise<IndexedProject[]>;
  close?(): Promise<void>;
}
