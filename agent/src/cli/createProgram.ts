import { readFile } from "node:fs/promises";
import { Command, InvalidArgumentError } from "commander";
import type { RegisteredProject } from "../config/projectRegistry.js";
import { GcalBackendError, type GcalBackendClient } from "../domain/GcalBackendClient.js";
import { formatCompactJson } from "../formatters/jsonFormatters.js";
import {
  formatBatchSourcesText,
  formatCandidateBlockText,
  formatCandidatesText,
  formatHydratedSearchText,
  formatSelectedMetadataText,
  formatSnippetManifestText,
  formatSourceText,
  formatTraceRowsText,
  formatTraceSectionText,
} from "../formatters/textFormatters.js";
import { contextAffordanceWarnings } from "../kernel/affordanceSignals.js";
import { callerTraceThresholdFromEnv } from "../kernel/inspectPolicy.js";
import { inboundTraceDecision } from "../kernel/tracePolicy.js";
import type { TraceEdge } from "../domain/types.js";
import {
  BATCH_SNIPPET_CHUNK_BYTES,
  BatchGetFailedError,
  getSymbols,
  type GetSymbolFailure,
  type GetSymbolSuccess,
} from "../workflows/getSymbols.js";
import {
  EnhancedSearchFailedError,
  MAX_SEARCH_CANDIDATES,
  searchSymbols,
} from "../workflows/searchSymbols.js";
import {
  DEFAULT_JS_WORKFLOW_MAX_CALLS,
  DEFAULT_JS_WORKFLOW_TIMEOUT_MS,
  formatJavaScriptWorkflowValue,
  runJavaScriptWorkflow,
} from "../workflows/runJavaScriptWorkflow.js";
import { writeLine, type WriteFn } from "./output.js";

export interface ProgramDeps {
  client: GcalBackendClient;
  initProject: (repoPath: string) => Promise<RegisteredProject>;
  writeOut: WriteFn;
  writeErr: WriteFn;
}

interface SearchCommandOptions {
  limit: number;
  label?: string;
  file?: string;
  qn?: string;
  query: string[];
  snippets?: number | true;
}

interface TraceCommandOptions {
  depth: number;
  limit: number;
}

interface InspectCommandOptions {
  limit: number;
}

interface GetCommandOptions {
  chunk?: number;
  expectedSourceSha?: string;
}

interface WorkflowCommandOptions {
  file?: string;
  js?: string;
  maxCalls: number;
  timeoutMs: number;
}

function numberOption(value: string): number {
  if (!/^(0|[1-9]\d*)$/.test(value)) {
    throw new InvalidArgumentError(`expected a non-negative integer, got ${value}`);
  }

  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed)) {
    throw new InvalidArgumentError(`expected a non-negative integer, got ${value}`);
  }

  return parsed;
}

function positiveNumberOption(value: string): number {
  const parsed = numberOption(value);
  if (parsed === 0) {
    throw new InvalidArgumentError(`expected a positive integer, got ${value}`);
  }
  return parsed;
}

function collectOption(value: string, previous: string[]): string[] {
  return [...previous, value];
}

function writeBoundedTrace(
  deps: ProgramDeps,
  command: "callers" | "callees",
  trace: TraceEdge[],
  limit: number,
): void {
  writeLine(deps.writeOut, formatTraceRowsText(trace.slice(0, limit)));
  if (trace.length > limit) {
    writeLine(
      deps.writeErr,
      `gcal: ${command} truncated to ${limit} of ${trace.length} rows; rerun with --limit ${trace.length}`,
    );
  }
}

export function createProgram(deps: ProgramDeps): Command {
  const program = new Command();
  program
    .name("gcal")
    .description("Goldeneye Code Agent Layer")
    .showHelpAfterError()
    .configureOutput({
      writeOut: deps.writeOut,
      writeErr: deps.writeErr,
    });

  program
    .command("search")
    .argument("<query>")
    .option("--limit <n>", "maximum rows", numberOption, 5)
    .option("--label <label>")
    .option("--file <regex>")
    .option("--qn <regex>")
    .option("--query <query>", "additional query branch", collectOption, [])
    .option("--snippets [n]", "hydrate top candidates", positiveNumberOption)
    .action(async (query: string, options: SearchCommandOptions) => {
      const searchOptions = {
        limit: options.limit,
        label: options.label,
        filePattern: options.file,
        qualifiedNamePattern: options.qn,
      };
      const snippetLimit = options.snippets === true ? 3 : options.snippets;

      if (options.query.length === 0 && snippetLimit === undefined) {
        const rows = await deps.client.search(query, searchOptions);
        writeLine(deps.writeOut, formatCandidatesText(rows));
        return;
      }

      if (snippetLimit !== undefined && snippetLimit > 5) {
        throw new Error("gcal search --snippets accepts at most 5");
      }

      const result = await searchSymbols(
        deps.client,
        [query, ...options.query],
        { ...searchOptions, limit: Math.min(options.limit, MAX_SEARCH_CANDIDATES) },
        snippetLimit,
      );
      const output =
        snippetLimit === undefined
          ? formatCandidatesText(result.candidates)
          : formatHydratedSearchText(result.candidates, result.snippets);
      if (output.length > 0) {
        writeLine(deps.writeOut, output);
      }

      for (const failure of result.queryFailures) {
        writeLine(
          deps.writeErr,
          `gcal: search query failed ${JSON.stringify(failure.query)}: ${failure.message}`,
        );
      }
      for (const failure of result.hydrationFailures) {
        writeLine(
          deps.writeErr,
          `gcal: search snippet failed ${JSON.stringify(failure.qualifiedName)}: ${failure.message}`,
        );
      }

      if (result.queryFailures.length > 0 || result.hydrationFailures.length > 0) {
        throw new EnhancedSearchFailedError(
          result.queryFailures.length,
          result.hydrationFailures.length,
        );
      }
    });

  program
    .command("symbol")
    .argument("<nameRegex>")
    .option("--limit <n>", "maximum rows", numberOption, 5)
    .option("--label <label>")
    .option("--file <regex>")
    .option("--qn <regex>")
    .action(async (nameRegex: string, options: SearchCommandOptions) => {
      const rows = await deps.client.symbol(nameRegex, {
        limit: options.limit,
        label: options.label,
        filePattern: options.file,
        qualifiedNamePattern: options.qn,
      });
      writeLine(deps.writeOut, formatCandidatesText(rows));
    });

  program
    .command("inspect")
    .argument("<queryOrQualifiedName>")
    .option("--limit <n>", "candidate search limit", numberOption, 5)
    .action(async (queryOrQualifiedName: string, options: InspectCommandOptions) => {
      const isQualifiedName = queryOrQualifiedName.includes(".");
      const candidates = isQualifiedName
        ? []
        : await deps.client.search(queryOrQualifiedName, { limit: options.limit });
      const selectedName = isQualifiedName ? queryOrQualifiedName : candidates[0]?.qualifiedName;

      if (selectedName === undefined) {
        throw new Error(`inspect found no candidates for ${queryOrQualifiedName}`);
      }

      const selected = await deps.client.get(selectedName);
      const warnings = contextAffordanceWarnings(selected);
      const inboundHint = inboundTraceDecision({
        qualifiedName: selected.qualifiedName,
        callerCount: selected.callers,
        threshold: callerTraceThresholdFromEnv(process.env),
      });
      const inbound =
        inboundHint ?? (await deps.client.callers(selected.qualifiedName, { depth: 1 }));
      const outbound = await deps.client.callees(selected.qualifiedName, { depth: 1 });
      const sections = [
        candidates.length > 0 ? formatCandidateBlockText(candidates) : "",
        formatSelectedMetadataText(selected, warnings),
        formatTraceSectionText("inbound", inbound),
        formatTraceSectionText("outbound", outbound),
      ].filter((section) => section.length > 0);

      writeLine(deps.writeOut, sections.join("\n\n"));
    });

  program
    .command("get")
    .argument("<qualifiedNames...>")
    .option("--chunk <n>", "fetch one bounded source chunk", positiveNumberOption)
    .option("--expected-source-sha <sha256>", "require exact lowercase source SHA-256")
    .action(async (qualifiedNames: string[], options: GetCommandOptions) => {
      validateGetChunkOptions(qualifiedNames, options);

      if (options.chunk !== undefined) {
        if (deps.client.getSnippetChunk === undefined) {
          throw new Error("gcal get --chunk requires Goldeneye snippet chunk support");
        }

        const selected = await deps.client.getSnippetChunk(qualifiedNames[0], {
          chunk: options.chunk,
          chunkBytes: BATCH_SNIPPET_CHUNK_BYTES,
          expectedSourceSha256: options.expectedSourceSha,
        });
        writeLine(deps.writeOut, formatSourceText(selected));
        return;
      }

      if (qualifiedNames.length === 1) {
        try {
          const selected = await deps.client.get(qualifiedNames[0]);
          writeLine(deps.writeOut, formatSourceText(selected));
        } catch (error) {
          if (
            !(error instanceof GcalBackendError) ||
            error.code !== "SnippetTooLarge" ||
            deps.client.getSnippetManifest === undefined
          ) {
            throw error;
          }

          const manifest = await deps.client.getSnippetManifest(
            qualifiedNames[0],
            BATCH_SNIPPET_CHUNK_BYTES,
          );
          writeLine(deps.writeOut, formatSnippetManifestText(manifest));
        }
        return;
      }

      const outcomes = await getSymbols(deps.client, qualifiedNames);
      const successes = outcomes.filter(
        (outcome): outcome is GetSymbolSuccess => outcome.status === "ok",
      );
      const failures = outcomes.filter(
        (outcome): outcome is GetSymbolFailure => outcome.status === "error",
      );

      const output = formatBatchSourcesText(successes.map((outcome) => outcome.selected));
      if (output.length > 0) {
        writeLine(deps.writeOut, output);
      }

      for (const failure of failures) {
        writeLine(deps.writeErr, `gcal: get failed ${failure.qualifiedName}: ${failure.message}`);
      }

      if (failures.length > 0) {
        throw new BatchGetFailedError(failures.length, outcomes.length);
      }
    });

  program
    .command("callers")
    .argument("<qualifiedName>")
    .option("--depth <n>", "trace depth", numberOption, 1)
    .option("--limit <n>", "maximum rows", numberOption, 20)
    .action(async (qualifiedName: string, options: TraceCommandOptions) => {
      const trace = await deps.client.callers(qualifiedName, { depth: options.depth });
      writeBoundedTrace(deps, "callers", trace, options.limit);
    });

  program
    .command("callees")
    .argument("<qualifiedName>")
    .option("--depth <n>", "trace depth", numberOption, 1)
    .option("--limit <n>", "maximum rows", numberOption, 20)
    .action(async (qualifiedName: string, options: TraceCommandOptions) => {
      const trace = await deps.client.callees(qualifiedName, { depth: options.depth });
      writeBoundedTrace(deps, "callees", trace, options.limit);
    });

  program
    .command("workflow")
    .description("run trusted JavaScript over bounded GCAL operations")
    .option("--js <code>", "execute JavaScript source")
    .option("--file <path>", "execute JavaScript from a file")
    .option(
      "--max-calls <n>",
      "maximum backend calls",
      positiveNumberOption,
      DEFAULT_JS_WORKFLOW_MAX_CALLS,
    )
    .option(
      "--timeout-ms <n>",
      "worker timeout in milliseconds",
      positiveNumberOption,
      DEFAULT_JS_WORKFLOW_TIMEOUT_MS,
    )
    .action(async (options: WorkflowCommandOptions) => {
      const code = await loadWorkflowCode(options);
      const result = await runJavaScriptWorkflow(deps.client, code, {
        maxCalls: options.maxCalls,
        timeoutMs: options.timeoutMs,
      });

      writeLine(deps.writeOut, formatJavaScriptWorkflowValue(result.value));
      if (result.stdout.length > 0) {
        writeLine(deps.writeErr, `gcal: workflow stdout\n${result.stdout.trimEnd()}`);
      }
      if (result.stderr.length > 0) {
        writeLine(deps.writeErr, `gcal: workflow stderr\n${result.stderr.trimEnd()}`);
      }
      if (result.logsTruncated) {
        writeLine(deps.writeErr, "gcal: workflow console output truncated");
      }
    });

  program.command("arch").action(async () => {
    writeLine(deps.writeOut, formatCompactJson(await deps.client.arch()));
  });

  program.command("status").action(async () => {
    writeLine(deps.writeOut, formatCompactJson(await deps.client.status()));
  });

  program
    .command("init")
    .description("index and register a project for cwd-based lookup")
    .argument("[repoPath]", "repository path", ".")
    .action(async (repoPath: string) => {
      const registered = await deps.initProject(repoPath);
      writeLine(
        deps.writeOut,
        formatCompactJson({ project: registered.project, root_path: registered.rootPath }),
      );
    });

  program
    .command("index")
    .argument("[repoPath]", "repository path", ".")
    .action(async (repoPath: string) => {
      writeLine(deps.writeOut, formatCompactJson(await deps.client.index(repoPath)));
    });

  for (const command of program.commands) {
    command.exitOverride();
  }

  return program;
}

async function loadWorkflowCode(options: WorkflowCommandOptions): Promise<string> {
  if (options.js !== undefined && options.file !== undefined) {
    throw new Error("gcal workflow accepts exactly one of --js or --file");
  }
  if (options.js !== undefined) return options.js;
  if (options.file !== undefined) return readFile(options.file, "utf8");
  throw new Error("gcal workflow requires --js <code> or --file <path>");
}

function validateGetChunkOptions(qualifiedNames: string[], options: GetCommandOptions): void {
  if (options.expectedSourceSha !== undefined) {
    if (options.chunk === undefined) {
      throw new Error("gcal get --expected-source-sha requires --chunk");
    }

    if (!/^[0-9a-f]{64}$/.test(options.expectedSourceSha)) {
      throw new Error(
        "gcal get --expected-source-sha must be exactly 64 lowercase hexadecimal characters",
      );
    }
  }

  if (options.chunk !== undefined && qualifiedNames.length !== 1) {
    throw new Error("gcal get --chunk accepts exactly one symbol");
  }
}
