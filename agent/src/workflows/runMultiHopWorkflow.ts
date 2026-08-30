import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import type {
  MultiHopWorkflowOptions,
  MultiHopWorkflowResult,
  SelectedSymbol,
  TraceEdge,
  WorkflowHop,
} from "../domain/types.js";
import { validateSearchQueries } from "./searchSymbols.js";

export const MAX_WORKFLOW_SEARCH_LIMIT = 20;
export const MAX_WORKFLOW_TRACE_LIMIT = 50;
export const MAX_WORKFLOW_DEPTH = 4;
export const WORKFLOW_SOURCE_CHUNK_BYTES = 8_192;

export class MultiHopWorkflowFailedError extends Error {
  constructor(readonly failures: number) {
    super(`gcal workflow failed for ${failures} hops`);
    this.name = "MultiHopWorkflowFailedError";
  }
}

export async function runMultiHopWorkflow(
  client: GcalBackendClient,
  queryOrQualifiedName: string,
  options: MultiHopWorkflowOptions,
): Promise<MultiHopWorkflowResult> {
  validateWorkflowOptions(queryOrQualifiedName, options);

  const candidates = options.exact
    ? []
    : await client.search(queryOrQualifiedName, { limit: options.searchLimit });
  const selectedQualifiedName = options.exact
    ? queryOrQualifiedName
    : candidates[options.rank - 1]?.qualifiedName;

  if (selectedQualifiedName === undefined) {
    throw new Error(
      `workflow found no candidate at rank ${options.rank} for ${queryOrQualifiedName}`,
    );
  }

  const hops: WorkflowHop[] = [
    ...(options.source ? (["source"] as const) : []),
    ...(options.callers ? (["callers"] as const) : []),
    ...(options.callees ? (["callees"] as const) : []),
  ];
  const settled = await Promise.allSettled(
    hops.map((hop) => executeHop(client, selectedQualifiedName, hop, options.depth)),
  );
  const result: MultiHopWorkflowResult = {
    candidates,
    selectedQualifiedName,
    failures: [],
  };

  for (const [index, outcome] of settled.entries()) {
    const hop = hops[index];
    if (outcome.status === "rejected") {
      result.failures.push({ hop, message: singleLineError(outcome.reason) });
      continue;
    }

    if (hop === "source") {
      result.source = outcome.value as SelectedSymbol;
    } else if (hop === "callers") {
      const trace = outcome.value as TraceEdge[];
      result.inbound = trace.slice(0, options.traceLimit);
      result.inboundTotal = trace.length;
    } else {
      const trace = outcome.value as TraceEdge[];
      result.outbound = trace.slice(0, options.traceLimit);
      result.outboundTotal = trace.length;
    }
  }

  return result;
}

function validateWorkflowOptions(
  queryOrQualifiedName: string,
  options: MultiHopWorkflowOptions,
): void {
  validateSearchQueries([queryOrQualifiedName]);

  if (!options.source && !options.callers && !options.callees) {
    throw new Error("gcal workflow requires --source, --callers, --callees, or --all");
  }
  if (options.searchLimit < 1 || options.searchLimit > MAX_WORKFLOW_SEARCH_LIMIT) {
    throw new Error(`gcal workflow --limit must be between 1 and ${MAX_WORKFLOW_SEARCH_LIMIT}`);
  }
  if (options.rank < 1 || options.rank > options.searchLimit) {
    throw new Error("gcal workflow --rank must be between 1 and --limit");
  }
  if (options.exact && options.rank !== 1) {
    throw new Error("gcal workflow --rank cannot be used with --exact");
  }
  if (options.depth < 1 || options.depth > MAX_WORKFLOW_DEPTH) {
    throw new Error(`gcal workflow --depth must be between 1 and ${MAX_WORKFLOW_DEPTH}`);
  }
  if (options.traceLimit < 1 || options.traceLimit > MAX_WORKFLOW_TRACE_LIMIT) {
    throw new Error(
      `gcal workflow --trace-limit must be between 1 and ${MAX_WORKFLOW_TRACE_LIMIT}`,
    );
  }
}

function executeHop(
  client: GcalBackendClient,
  qualifiedName: string,
  hop: WorkflowHop,
  depth: number,
): Promise<SelectedSymbol | TraceEdge[]> {
  if (hop === "source") {
    return client.getSnippetChunk === undefined
      ? client.get(qualifiedName)
      : client.getSnippetChunk(qualifiedName, {
          chunk: 1,
          chunkBytes: WORKFLOW_SOURCE_CHUNK_BYTES,
        });
  }
  if (hop === "callers") {
    return client.callers(qualifiedName, { depth });
  }
  return client.callees(qualifiedName, { depth });
}

function singleLineError(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/\s+/g, " ").trim() || "unknown error";
}
