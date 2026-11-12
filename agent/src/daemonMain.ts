#!/usr/bin/env node
import { daemonLockPath } from "./adapters/daemon/daemonEndpoint.js";
import { StdioGoldeneyeClient } from "./adapters/goldeneye/StdioGoldeneyeClient.js";
import { startDaemonServer } from "./daemon/startDaemonServer.js";

try {
  const options = parseOptions(process.argv.slice(2));
  const daemon = await startDaemonServer({
    endpoint: options.endpoint,
    lockPath: daemonLockPath(options.gcalHome),
    idleTimeoutMs: options.idleTimeoutMs,
    createClient: (command, project) => new StdioGoldeneyeClient({ command, project }),
  });

  const shutdown = () => {
    void daemon.close();
  };
  process.once("SIGINT", shutdown);
  process.once("SIGTERM", shutdown);
  await daemon.closed;
} catch (error) {
  process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
}

interface DaemonMainOptions {
  endpoint: string;
  gcalHome: string;
  idleTimeoutMs: number;
}

function parseOptions(args: string[]): DaemonMainOptions {
  const endpoint = option(args, "--endpoint");
  const gcalHome = option(args, "--gcal-home");
  const idleTimeout = option(args, "--idle-ms");
  const idleTimeoutMs = Number(idleTimeout);

  if (!endpoint || !gcalHome || !Number.isSafeInteger(idleTimeoutMs) || idleTimeoutMs <= 0) {
    throw new Error("invalid GCAL daemon startup arguments");
  }
  return { endpoint, gcalHome, idleTimeoutMs };
}

function option(args: string[], name: string): string | undefined {
  const index = args.indexOf(name);
  return index < 0 ? undefined : args[index + 1];
}
