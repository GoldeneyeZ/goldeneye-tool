import { CommanderError } from "commander";
import { homedir } from "node:os";
import { join } from "node:path";
import { resolveRegisteredProject } from "../config/projectRegistry.js";
import type { GcalBackendClient } from "../domain/GcalBackendClient.js";
import { initializeProject } from "../workflows/initializeProject.js";
import { BatchGetFailedError } from "../workflows/getSymbols.js";
import { EnhancedSearchFailedError } from "../workflows/searchSymbols.js";
import { createProgram } from "./createProgram.js";
import type { WriteFn } from "./output.js";

const DEFAULT_GOLDENEYE_COMMAND = "goldeneye";
const PROJECT_COMMANDS = new Set([
  "arch",
  "callees",
  "callers",
  "get",
  "inspect",
  "search",
  "status",
  "symbol",
]);

export interface ClientConfig {
  backend: "goldeneye" | "benchmark";
  command: string;
  mcpUrl: string | undefined;
  project: string;
}

export interface RunCliOptions {
  argv: string[];
  env: Record<string, string | undefined>;
  createClient: (config: ClientConfig) => GcalBackendClient;
  writeOut: WriteFn;
  writeErr: WriteFn;
  currentDirectory?: string;
  homeDir?: string;
  resolveProject?: typeof resolveRegisteredProject;
}

export async function runCli(options: RunCliOptions): Promise<number> {
  const args = options.argv.slice(2);
  const commandName = firstCommandName(args);
  const helpRequest = isHelpRequest(args);
  const currentDirectory = options.currentDirectory ?? process.cwd();
  const gcalHome = options.env.GCAL_HOME ?? join(options.homeDir ?? homedir(), ".gcal");
  let project = options.env.GCAL_PROJECT;

  if (helpRequest) {
    return renderHelp(args, options);
  }

  const backend = options.env.GCAL_BACKEND ?? "goldeneye";
  if (backend !== "goldeneye" && backend !== "benchmark") {
    options.writeErr("GCAL_BACKEND must be 'goldeneye' or 'benchmark'\n");
    return 2;
  }
  if (backend === "benchmark" && !options.env.GCAL_MCP_COMMAND) {
    options.writeErr("GCAL_MCP_COMMAND is required when GCAL_BACKEND=benchmark\n");
    return 2;
  }

  if (!project && commandName !== undefined && PROJECT_COMMANDS.has(commandName)) {
    try {
      project = await (options.resolveProject ?? resolveRegisteredProject)(
        currentDirectory,
        gcalHome,
      );
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      options.writeErr(`${message}\n`);
      return 1;
    }

    if (!project) {
      options.writeErr(
        `No GCAL project registered for ${currentDirectory}; run 'gcal init' in the project root\n`,
      );
      return 2;
    }
  }

  const client = options.createClient({
    backend,
    command:
      backend === "goldeneye"
        ? (options.env.GCAL_GOLDENEYE_COMMAND ?? DEFAULT_GOLDENEYE_COMMAND)
        : options.env.GCAL_MCP_COMMAND!,
    mcpUrl: backend === "benchmark" ? options.env.GCAL_MCP_URL : undefined,
    project: project ?? "",
  });
  const program = createProgram({
    client,
    initProject: (repoPath) => initializeProject(client, repoPath, currentDirectory, gcalHome),
    writeOut: options.writeOut,
    writeErr: options.writeErr,
  });
  program.exitOverride();

  try {
    await program.parseAsync(options.argv);
    return 0;
  } catch (error) {
    if (error instanceof CommanderError) {
      return error.exitCode;
    }

    if (error instanceof BatchGetFailedError || error instanceof EnhancedSearchFailedError) {
      return 1;
    }

    const message = error instanceof Error ? error.message : String(error);
    options.writeErr(`${message}\n`);
    return 1;
  } finally {
    await client.close?.();
  }
}

function isHelpRequest(args: string[]): boolean {
  return args[0] === "help" || args.includes("--help") || args.includes("-h");
}

function renderHelp(args: string[], options: RunCliOptions): number {
  const program = createProgram({
    client: createHelpOnlyClient(),
    initProject: async () => {
      throw new Error("client is not available during help rendering");
    },
    writeOut: options.writeOut,
    writeErr: options.writeErr,
  });
  const commandName = helpCommandName(args);
  const command =
    commandName === undefined
      ? program
      : program.commands.find((item) => item.name() === commandName);

  if (command === undefined) {
    options.writeErr(`Unknown command: ${commandName}\n`);
    return 1;
  }

  options.writeOut(command.helpInformation());
  return 0;
}

function helpCommandName(args: string[]): string | undefined {
  if (args[0] === "help") {
    return args[1];
  }

  return firstCommandName(args);
}

function firstCommandName(args: string[]): string | undefined {
  for (const arg of args) {
    if (!arg.startsWith("-")) {
      return arg;
    }
  }

  return undefined;
}

function createHelpOnlyClient(): GcalBackendClient {
  const fail = async () => {
    throw new Error("client is not available during help rendering");
  };

  return {
    search: fail,
    symbol: fail,
    get: fail,
    callers: fail,
    callees: fail,
    arch: fail,
    status: fail,
    index: fail,
    projects: fail,
  };
}
