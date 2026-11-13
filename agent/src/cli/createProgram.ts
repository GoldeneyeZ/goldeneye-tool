import { readFile } from "node:fs/promises";
import { Command, InvalidArgumentError, Option } from "commander";
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
import { projectRoot } from "../kernel/codeGraph.js";
import { FreshnessProbe } from "../kernel/freshness.js";
import { callerTraceThresholdFromEnv } from "../kernel/inspectPolicy.js";
import { parseLocator } from "../kernel/locator.js";
import { NewFiles } from "../kernel/newFiles.js";
import { SourceReader } from "../kernel/sourceFiles.js";
import { inboundTraceDecision } from "../kernel/tracePolicy.js";
import type { TraceEdge } from "../domain/types.js";
import { checkSyntax } from "../workflows/checkSyntax.js";
import {
  architecture,
  callees,
  globPattern,
  grepCode,
  implementations,
  references,
  searchCode,
  testsUsing,
} from "../workflows/exploreCode.js";
import { normalizeFilePattern } from "../workflows/filePattern.js";
import {
  BATCH_SNIPPET_CHUNK_BYTES,
  BatchGetFailedError,
  getSymbols,
  type GetSymbolFailure,
  type GetSymbolSuccess,
} from "../workflows/getSymbols.js";
import { readLocators, renderOutline, type CodeContext, type ReadResult } from "../workflows/readCode.js";
import { replaceText } from "../workflows/replaceText.js";
import { resolveSymbol } from "../workflows/resolveSymbol.js";
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
  /** Backend project name; empty while no project is selected. */
  project?: string;
  /** Root directory of the selected project from the local registry, when registered. */
  registeredRoot?: () => Promise<string | undefined>;
  initProject: (repoPath: string) => Promise<RegisteredProject>;
  writeOut: WriteFn;
  writeErr: WriteFn;
}

/** The command already printed its findings; only the exit status is left. */
export class CommandFailedError extends Error {
  constructor() {
    super("gcal command reported a failure");
    this.name = "CommandFailedError";
  }
}

interface SearchCommandOptions {
  limit: number;
  label?: string;
  file?: string;
  path?: string;
  qn?: string;
  query: string[];
  snippets?: number | true;
  context?: number;
}

interface TraceCommandOptions {
  depth: number;
  limit?: number;
  text: boolean;
}

interface InspectCommandOptions {
  limit: number;
}

interface GetCommandOptions {
  chunk?: number;
  expectedSourceSha?: string;
  full?: boolean;
  tests?: boolean;
}

interface GrepCommandOptions {
  fixedStrings: boolean;
  file?: string;
  glob?: string;
  path?: string;
  context?: number;
  limit: number;
}

const HELP_EXAMPLES = `
Point at code any natural way: Type, Type.member, pkg.Type.member, path/File.ext,
File.ext:42 (symbol at that line), File.ext:10-60 (raw lines).

  gcal search BookingService cancelBooking      symbols by name; falls back to text
  gcal get BookingService.cancel Repo.find      source, several at once
  gcal get BookingService                       whole type; an outline above 500 lines (--full: whole anyway)
  gcal get src/app/Booking.java:120-180         raw line range
  gcal grep cancel Booking.java BookingTests.java -C 8   matching lines with context, like rg -C
  gcal callers BookingService.cancel            call sites (graph + text references)
  gcal callees BookingService.cancel --depth 2
  gcal impls PaymentGateway                     implementations and subclasses
  gcal grep "retry\\.max|retry\\.delay"         text search (regex, like rg), grouped by symbol
  gcal replace "cancel\\(" "cancelBooking(" src   the same edit in every file; prints the changed lines
  gcal arch                                    directory layout by file count
  gcal workflow --file plan.js                  JavaScript over search, source and traces, for dependent hops

Chain several commands in one shell call when you need more than one answer.`;

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

/** Graph-backed commands need raw tool access; the benchmark adapter has none. */
async function codeContext(deps: ProgramDeps): Promise<CodeContext | undefined> {
  const backend = deps.client;
  if (backend.tool === undefined || !deps.project) return undefined;

  const client = { tool: (name: string, args: Record<string, unknown>) => backend.tool!(name, args) };
  // The local registry already knows the root; asking the backend costs a
  // round trip on every command and is only needed for unregistered projects.
  const root = (await deps.registeredRoot?.()) ?? (await projectRoot(client));
  return {
    client,
    project: deps.project,
    root,
    reader: new SourceReader(root),
    freshness: new FreshnessProbe(client),
    newFiles: new NewFiles(root),
  };
}

// A second call to see the rows a first one cut costs far more than the rows:
// a refactor needs every caller, and most symbols have fewer than this.
const GRAPH_TRACE_ROWS = 60;
const RAW_TRACE_ROWS = 20;

function emit(deps: ProgramDeps, result: ReadResult): void {
  if (result.text.length > 0) writeLine(deps.writeOut, result.text);
  if (!result.ok) throw new CommandFailedError();
}

/**
 * Prints the result of a lookup. "Not found", "ambiguous" and "no matches" are
 * answers, not failures: agents chain lookups with `&&`, and a failing exit
 * code would skip the commands that follow.
 */
function answer(deps: ProgramDeps, result: ReadResult): void {
  if (result.text.length > 0) writeLine(deps.writeOut, result.text);
}

function pathAlias(): Option {
  return new Option("--path <regex>", "alias of --file").hideHelp();
}

/** `-C n` belongs to grep; agents also pass it to search and get. */
function grepContextAlias(): Option {
  return new Option("-C, --context <n>", "as in gcal grep").argParser(numberOption).hideHelp();
}

export function createProgram(deps: ProgramDeps): Command {
  const program = new Command();
  program
    .name("gcal")
    .description("Goldeneye Code Agent Layer: compact code discovery over the Goldeneye graph")
    .showHelpAfterError()
    .addHelpText("after", HELP_EXAMPLES)
    .configureOutput({
      writeOut: deps.writeOut,
      writeErr: deps.writeErr,
    });

  program
    .command("search")
    .description("find symbols by name; a query with no symbol match is searched as text")
    .argument("<queries...>")
    .option("--limit <n>", "maximum rows", numberOption, 5)
    .option("--label <label>", "symbol kind, e.g. Class, Method, Field")
    .option("--file <regex-or-glob>", "restrict to matching file paths")
    .addOption(pathAlias())
    .option("--qn <regex>", "restrict to matching qualified names")
    .option("--query <query>", "additional query", collectOption, [])
    .option("--snippets [n]", "append source of the top results", positiveNumberOption)
    .addOption(grepContextAlias())
    .action(async (queries: string[], options: SearchCommandOptions) => {
      const filePattern = normalizeFilePattern(options.file ?? options.path);
      // `search <pattern> [paths...] -C n` is a grep in every way but its name.
      if (options.context !== undefined) {
        const context = await requireContext(deps, "search -C");
        answer(
          deps,
          await grepCode(context, {
            pattern: queries[0],
            regex: true,
            filePattern,
            paths: queries.slice(1),
            context: options.context,
            limit: 30,
          }),
        );
        return;
      }
      const snippetLimit = options.snippets === true ? 3 : options.snippets;
      if (snippetLimit !== undefined && snippetLimit > 5) {
        throw new Error("gcal search --snippets accepts at most 5");
      }

      const context = await codeContext(deps);
      if (context !== undefined) {
        answer(
          deps,
          await searchCode(context, deps.client, {
            queries: [...queries, ...options.query],
            limit: options.limit,
            label: options.label,
            filePattern,
            qualifiedNamePattern: options.qn,
            snippets: snippetLimit,
          }),
        );
        return;
      }

      const searchOptions = {
        limit: options.limit,
        label: options.label,
        filePattern,
        qualifiedNamePattern: options.qn,
      };
      const allQueries = [...queries, ...options.query];
      if (allQueries.length === 1 && snippetLimit === undefined) {
        const rows = await deps.client.search(allQueries[0], searchOptions);
        writeLine(deps.writeOut, formatCandidatesText(rows));
        return;
      }

      const result = await searchSymbols(
        deps.client,
        allQueries,
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
    .command("grep")
    .description(
      "text search grouped by enclosing symbol; the pattern is a regex as in rg, or literal text when it is not one",
    )
    .argument("<pattern>")
    .argument("[paths...]", "files or directories to search in")
    .option("-F, --fixed-strings", "treat the pattern as literal text only", false)
    .addOption(new Option("--regex", "default; kept for compatibility").hideHelp())
    .option("-C, --context <n>", "print matching lines with n lines of context", numberOption)
    .option("--file <regex>", "restrict to matching file paths")
    .option("-g, --glob <glob>", "restrict to file paths matching a glob, as in rg: -g '*.py'")
    .addOption(pathAlias())
    .option("--limit <n>", "maximum matching symbols", positiveNumberOption, 30)
    .action(async (pattern: string, paths: string[], options: GrepCommandOptions) => {
      const context = await requireContext(deps, "grep");
      answer(
        deps,
        await grepCode(context, {
          pattern,
          regex: !options.fixedStrings,
          filePattern:
            normalizeFilePattern(options.file ?? options.path) ??
            (options.glob === undefined ? undefined : globPattern(options.glob)),
          paths,
          context: options.context,
          limit: options.limit,
        }),
      );
    });

  program
    .command("replace")
    .description(
      "replace text in working-tree files and print every changed line; the pattern is a regex as in gcal grep ($1 for groups), or literal text with -F",
    )
    .argument("<pattern>")
    .argument("<replacement>")
    .argument("<paths...>", "files or directories to rewrite; . for the whole repository")
    .option("-F, --fixed-strings", "treat pattern and replacement as literal text", false)
    .option("--dry-run", "print what would change and write nothing", false)
    .action(
      async (
        pattern: string,
        replacement: string,
        paths: string[],
        options: { fixedStrings: boolean; dryRun: boolean },
      ) => {
        const context = await requireContext(deps, "replace");
        emit(
          deps,
          await replaceText(context, {
            pattern,
            replacement,
            paths,
            literal: options.fixedStrings,
            dryRun: options.dryRun,
          }),
        );
      },
    );

  program
    .command("check")
    .description(
      "parse the files that differ from HEAD (or the given files and directories) and print the lines with syntax errors",
    )
    .argument("[paths...]", "files or directories; default: every file that differs from HEAD")
    .action(async (paths: string[]) => {
      const context = await requireContext(deps, "check");
      emit(deps, await checkSyntax(context, { paths }));
    });

  program
    .command("symbol")
    .description("find symbols whose name matches a regular expression")
    .argument("<nameRegex>")
    .option("--limit <n>", "maximum rows", numberOption, 5)
    .option("--label <label>")
    .option("--file <regex>")
    .addOption(pathAlias())
    .option("--qn <regex>")
    .action(async (nameRegex: string, options: SearchCommandOptions) => {
      const rows = await deps.client.symbol(nameRegex, {
        limit: options.limit,
        label: options.label,
        filePattern: options.file ?? options.path,
        qualifiedNamePattern: options.qn,
      });
      writeLine(deps.writeOut, formatCandidatesText(rows));
    });

  program
    .command("inspect")
    .description("metadata plus direct callers and callees of one symbol")
    .argument("<queryOrQualifiedName>")
    .option("--limit <n>", "candidate search limit", numberOption, 5)
    .action(async (queryOrQualifiedName: string, options: InspectCommandOptions) => {
      const context = await codeContext(deps);
      if (context !== undefined) {
        const inbound = await references(context, deps.client, {
          target: queryOrQualifiedName,
          depth: 1,
          limit: 10,
          includeText: false,
        });
        if (!inbound.ok) {
          answer(deps, inbound);
          return;
        }
        const outbound = await callees(context, deps.client, {
          target: queryOrQualifiedName,
          depth: 1,
          limit: 10,
        });
        emit(deps, { ok: true, text: `${inbound.text}\n\n${outbound.text}` });
        return;
      }

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
    .description(
      "source by symbol, file, file:line or file:start-end; types and files above 500 lines print an outline",
    )
    .argument("<locators...>")
    .option("--full", "whole source even for large types and files")
    .option("--tests", "also print the tests that use each named symbol")
    .option("--chunk <n>", "fetch one bounded source chunk", positiveNumberOption)
    .option("--expected-source-sha <sha256>", "require exact lowercase source SHA-256")
    // Accepted and ignored: get already prints whole symbols.
    .addOption(grepContextAlias())
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

      const context = await codeContext(deps);
      if (context !== undefined) {
        const read = await readLocators(context, qualifiedNames, {
          detail: options.full ? "full" : "auto",
        });
        if (options.tests) {
          const sections = [read.text];
          for (const name of qualifiedNames) {
            try {
              const tests = await testsUsing(context, name);
              if (tests !== undefined) sections.push(tests);
            } catch (error) {
              // The source above is already read; a failed test lookup must not discard it.
              const reason = error instanceof Error ? error.message.split("\n")[0] : String(error);
              sections.push(`# tests using ${name}: lookup failed (${reason}); try: gcal callers ${name}`);
            }
          }
          read.text = sections.join("\n\n");
        }
        answer(deps, read);
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
    .command("outline")
    .description("member list of a type, or symbol list of a file")
    .argument("<typeOrFile>")
    .action(async (target: string) => {
      const context = await requireContext(deps, "outline");
      const locator = parseLocator(target);
      if (locator.kind === "symbol") {
        const resolution = await resolveSymbol(context.client, context.project, locator);
        if (resolution.kind === "resolved") {
          emit(deps, { ok: true, text: await renderOutline(context, resolution.nodes[0]) });
          return;
        }
      }
      answer(deps, await readLocators(context, [target], { detail: "outline" }));
    });

  program
    .command("callers")
    .alias("refs")
    .description("call sites of a symbol: graph edges plus text references")
    .argument("<symbol>")
    .option("--depth <n>", "trace depth", numberOption, 1)
    .option("--limit <n>", "maximum rows (default: 60 from the graph, 20 otherwise)", numberOption)
    .option("--no-text", "graph edges only")
    .action(async (qualifiedName: string, options: TraceCommandOptions) => {
      const context = await codeContext(deps);
      if (context !== undefined) {
        answer(
          deps,
          await references(context, deps.client, {
            target: qualifiedName,
            depth: options.depth,
            limit: options.limit ?? GRAPH_TRACE_ROWS,
            includeText: options.text,
          }),
        );
        return;
      }

      const trace = await deps.client.callers(qualifiedName, { depth: options.depth });
      writeBoundedTrace(deps, "callers", trace, options.limit ?? RAW_TRACE_ROWS);
    });

  program
    .command("callees")
    .description("what a symbol calls")
    .argument("<symbol>")
    .option("--depth <n>", "trace depth", numberOption, 1)
    .option("--limit <n>", "maximum rows (default: 60 from the graph, 20 otherwise)", numberOption)
    .action(async (qualifiedName: string, options: TraceCommandOptions) => {
      const context = await codeContext(deps);
      if (context !== undefined) {
        answer(
          deps,
          await callees(context, deps.client, {
            target: qualifiedName,
            depth: options.depth,
            limit: options.limit ?? GRAPH_TRACE_ROWS,
          }),
        );
        return;
      }

      const trace = await deps.client.callees(qualifiedName, { depth: options.depth });
      writeBoundedTrace(deps, "callees", trace, options.limit ?? RAW_TRACE_ROWS);
    });

  program
    .command("impls")
    .alias("subtypes")
    .description("implementations and subclasses of a type")
    .argument("<type>")
    .option("--depth <n>", "inheritance levels to follow", positiveNumberOption, 1)
    .action(async (target: string, options: { depth: number }) => {
      const context = await requireContext(deps, "impls");
      answer(deps, await implementations(context, target, Math.min(options.depth, 4)));
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

  program
    .command("arch")
    .description("directory layout by indexed file count")
    .action(async () => {
      const context = await codeContext(deps);
      if (context !== undefined) {
        emit(deps, await architecture(context));
        return;
      }
      writeLine(deps.writeOut, formatCompactJson(await deps.client.arch()));
    });

  program
    .command("status")
    .description("index state of the current project")
    .action(async () => {
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
    .description("re-index a repository")
    .argument("[repoPath]", "repository path", ".")
    .action(async (repoPath: string) => {
      writeLine(deps.writeOut, formatCompactJson(await deps.client.index(repoPath)));
    });

  for (const command of program.commands) {
    command.exitOverride();
  }

  return program;
}

async function requireContext(deps: ProgramDeps, command: string): Promise<CodeContext> {
  const context = await codeContext(deps);
  if (context === undefined) {
    throw new Error(`gcal ${command} requires the Goldeneye backend and a selected project`);
  }
  return context;
}

async function loadWorkflowCode(options: WorkflowCommandOptions): Promise<string> {
  if (options.js !== undefined && options.file !== undefined) {
    throw new Error("gcal workflow accepts exactly one of --js or --file");
  }
  if (options.js !== undefined) return options.js;
  if (options.file !== undefined) return readFile(options.file, "utf8");
  throw new Error("gcal workflow requires --js <code> or --file <path>");
}

function validateGetChunkOptions(
  qualifiedNames: string[],
  options: GetCommandOptions,
): void {
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
