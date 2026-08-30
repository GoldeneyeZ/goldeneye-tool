import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import type { HydratedSymbolCandidate, SearchOptions, SymbolCandidate } from "../domain/types.js";

export const MAX_SEARCH_QUERIES = 8;
export const MAX_SEARCH_QUERY_BYTES = 512;
export const MAX_SEARCH_CANDIDATES = 20;
export const SEARCH_SNIPPET_CHUNK_BYTES = 4_096;

export interface SearchBranchFailure {
  query: string;
  message: string;
}

export interface SearchHydrationFailure {
  qualifiedName: string;
  message: string;
}

export interface SearchSymbolsResult {
  candidates: SymbolCandidate[];
  snippets: HydratedSymbolCandidate[];
  queryFailures: SearchBranchFailure[];
  hydrationFailures: SearchHydrationFailure[];
}

export class EnhancedSearchFailedError extends Error {
  constructor(
    readonly queryFailures: number,
    readonly hydrationFailures: number,
  ) {
    super(
      `gcal search failed for ${queryFailures} queries and ${hydrationFailures} snippet hydrations`,
    );
    this.name = "EnhancedSearchFailedError";
  }
}

export function validateSearchQueries(queries: string[]): void {
  if (queries.length > MAX_SEARCH_QUERIES) {
    throw new Error(`gcal search accepts at most ${MAX_SEARCH_QUERIES} query branches`);
  }

  for (const query of queries) {
    if (query.trim().length === 0) {
      throw new Error("gcal search enhanced query branches must not be empty or whitespace");
    }

    if (/[\t\r\n]/.test(query)) {
      throw new Error("gcal search query branches must not contain tabs or line breaks");
    }

    if (Buffer.byteLength(query, "utf8") > MAX_SEARCH_QUERY_BYTES) {
      throw new Error(
        `gcal search query branches must not exceed ${MAX_SEARCH_QUERY_BYTES} UTF-8 bytes`,
      );
    }
  }
}

export async function searchSymbols(
  client: GcalBackendClient,
  queries: string[],
  options: Partial<SearchOptions>,
  snippetLimit: number | undefined,
): Promise<SearchSymbolsResult> {
  validateSearchQueries(queries);
  const uniqueQueries = stableUnique(queries);
  const globalLimit = Math.min(options.limit ?? MAX_SEARCH_CANDIDATES, MAX_SEARCH_CANDIDATES);
  const branchResults: SymbolCandidate[][] = [];
  const queryFailures: SearchBranchFailure[] = [];

  for (const query of uniqueQueries) {
    try {
      const rows = await client.search(query, { ...options, limit: globalLimit });
      branchResults.push(rows);
    } catch (error) {
      queryFailures.push({ query, message: singleLineError(error) });
    }
  }

  const boundedCandidates = mergeRankMajor(branchResults, globalLimit);
  const snippets: HydratedSymbolCandidate[] = [];
  const hydrationFailures: SearchHydrationFailure[] = [];

  for (const candidate of boundedCandidates.slice(0, snippetLimit ?? 0)) {
    try {
      snippets.push({
        candidate,
        selected:
          client.getSnippetChunk === undefined
            ? await client.get(candidate.qualifiedName)
            : await client.getSnippetChunk(candidate.qualifiedName, {
                chunk: 1,
                chunkBytes: SEARCH_SNIPPET_CHUNK_BYTES,
              }),
      });
    } catch (error) {
      hydrationFailures.push({
        qualifiedName: candidate.qualifiedName,
        message: singleLineError(error),
      });
    }
  }

  return {
    candidates: boundedCandidates,
    snippets,
    queryFailures,
    hydrationFailures,
  };
}

function mergeRankMajor(
  branchResults: SymbolCandidate[][],
  globalLimit: number,
): SymbolCandidate[] {
  const merged: SymbolCandidate[] = [];
  const seenQualifiedNames = new Set<string>();
  const maxRank = Math.max(0, ...branchResults.map((rows) => rows.length));

  for (let rank = 0; rank < maxRank && merged.length < globalLimit; rank += 1) {
    for (const rows of branchResults) {
      const candidate = rows[rank];
      if (candidate !== undefined && !seenQualifiedNames.has(candidate.qualifiedName)) {
        seenQualifiedNames.add(candidate.qualifiedName);
        merged.push(candidate);
        if (merged.length === globalLimit) {
          break;
        }
      }
    }
  }

  return merged;
}

function stableUnique(values: string[]): string[] {
  return [...new Set(values)];
}

function singleLineError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/\s+/g, " ").trim() || "unknown error";
}
