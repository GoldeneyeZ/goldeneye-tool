import type { GcalBackendClient } from "../../domain/GcalBackendClient.js";
import { resolveDaemonConfig } from "../../config/daemonConfig.js";
import { DaemonGcalBackendClient } from "../daemon/DaemonGcalBackendClient.js";
import { StdioGoldeneyeClient } from "./StdioGoldeneyeClient.js";

export interface CreateGoldeneyeClientOptions {
  gcalHome: string;
  command: string;
  project: string;
  env: Record<string, string | undefined>;
  writeWarning(message: string): void;
}

export function createGoldeneyeClient(options: CreateGoldeneyeClientOptions): GcalBackendClient {
  const resolved = resolveDaemonConfig(options.gcalHome, options.env);
  for (const warning of resolved.warnings) {
    options.writeWarning(`${warning}\n`);
  }

  const directFactory = () =>
    new StdioGoldeneyeClient({
      command: options.command,
      project: options.project,
    });
  if (resolved.config.mode === "off") return directFactory();

  return new DaemonGcalBackendClient({
    gcalHome: options.gcalHome,
    command: options.command,
    project: options.project,
    idleTimeoutMs: resolved.config.idleTimeoutMs,
    fallbackFactory: directFactory,
    writeWarning: options.writeWarning,
  });
}
