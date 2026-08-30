import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import type { SelectedSymbol } from "../domain/types.js";

export const MAX_BATCH_GET_SYMBOLS = 32;
export const MAX_BATCH_QUALIFIED_NAME_BYTES = 512;
export const BATCH_SNIPPET_CHUNK_BYTES = 8_192;

export interface GetSymbolSuccess {
  status: "ok";
  qualifiedName: string;
  selected: SelectedSymbol;
}

export interface GetSymbolFailure {
  status: "error";
  qualifiedName: string;
  message: string;
}

export type GetSymbolOutcome = GetSymbolSuccess | GetSymbolFailure;

export class BatchGetFailedError extends Error {
  constructor(
    readonly failed: number,
    readonly total: number,
  ) {
    super(`gcal get failed for ${failed} of ${total} symbols`);
    this.name = "BatchGetFailedError";
  }
}

export function validateBatchQualifiedNames(qualifiedNames: string[]): void {
  if (qualifiedNames.length > MAX_BATCH_GET_SYMBOLS) {
    throw new Error(`gcal get accepts at most ${MAX_BATCH_GET_SYMBOLS} symbols per batch`);
  }

  for (const qualifiedName of qualifiedNames) {
    if (/[\t\r\n]/.test(qualifiedName)) {
      throw new Error("gcal get batch symbol IDs must not contain tabs or line breaks");
    }

    if (Buffer.byteLength(qualifiedName, "utf8") > MAX_BATCH_QUALIFIED_NAME_BYTES) {
      throw new Error(
        `gcal get batch symbol IDs must not exceed ${MAX_BATCH_QUALIFIED_NAME_BYTES} UTF-8 bytes`,
      );
    }
  }
}

export async function getSymbols(
  client: GcalBackendClient,
  qualifiedNames: string[],
): Promise<GetSymbolOutcome[]> {
  validateBatchQualifiedNames(qualifiedNames);
  const outcomes: GetSymbolOutcome[] = [];

  for (const qualifiedName of qualifiedNames) {
    try {
      outcomes.push({
        status: "ok",
        qualifiedName,
        selected:
          client.getSnippetChunk === undefined
            ? await client.get(qualifiedName)
            : await client.getSnippetChunk(qualifiedName, {
                chunk: 1,
                chunkBytes: BATCH_SNIPPET_CHUNK_BYTES,
              }),
      });
    } catch (error) {
      outcomes.push({
        status: "error",
        qualifiedName,
        message: singleLineError(error),
      });
    }
  }

  return outcomes;
}

function singleLineError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/\s+/g, " ").trim() || "unknown error";
}
