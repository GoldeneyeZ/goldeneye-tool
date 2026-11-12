import { readFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";

export const DEFAULT_DAEMON_IDLE_TIMEOUT_MS = 10 * 60 * 1_000;

export interface DaemonConfig {
  mode: "auto" | "off";
  idleTimeoutMs: number;
}

export interface ResolvedDaemonConfig {
  config: DaemonConfig;
  warnings: string[];
}

const daemonFileSchema = z
  .object({
    daemon: z
      .object({
        mode: z.enum(["auto", "off"]).optional(),
        idleTimeout: z.string().optional(),
      })
      .optional(),
  })
  .passthrough();

export function resolveDaemonConfig(
  gcalHome: string,
  env: Record<string, string | undefined>,
): ResolvedDaemonConfig {
  const warnings: string[] = [];
  let mode: DaemonConfig["mode"] = "auto";
  let idleTimeoutMs = DEFAULT_DAEMON_IDLE_TIMEOUT_MS;
  const configPath = join(gcalHome, "config.json");

  try {
    const parsed = daemonFileSchema.parse(JSON.parse(readFileSync(configPath, "utf8")));
    mode = parsed.daemon?.mode ?? mode;
    if (parsed.daemon?.idleTimeout !== undefined) {
      idleTimeoutMs = parseIdleTimeout(parsed.daemon.idleTimeout);
    }
  } catch (error) {
    if (!isMissingFile(error)) {
      warnings.push(`Ignoring invalid GCAL daemon config ${configPath}: ${errorMessage(error)}`);
    }
  }

  if (env.GCAL_DAEMON !== undefined) {
    if (env.GCAL_DAEMON === "auto" || env.GCAL_DAEMON === "off") {
      mode = env.GCAL_DAEMON;
    } else {
      warnings.push(`Ignoring invalid GCAL_DAEMON=${env.GCAL_DAEMON}; expected 'auto' or 'off'`);
    }
  }

  if (env.GCAL_DAEMON_IDLE !== undefined) {
    try {
      idleTimeoutMs = parseIdleTimeout(env.GCAL_DAEMON_IDLE);
    } catch (error) {
      warnings.push(
        `Ignoring invalid GCAL_DAEMON_IDLE=${env.GCAL_DAEMON_IDLE}: ${errorMessage(error)}`,
      );
    }
  }

  return { config: { mode, idleTimeoutMs }, warnings };
}

export function parseIdleTimeout(value: string): number {
  const match = /^([1-9]\d*)(ms|s|m|h)$/.exec(value);
  if (!match) {
    throw new Error("expected a positive duration such as '500ms', '30s', '10m', or '1h'");
  }

  const amount = Number(match[1]);
  const unit = match[2];
  const multiplier = unit === "ms" ? 1 : unit === "s" ? 1_000 : unit === "m" ? 60_000 : 3_600_000;
  const milliseconds = amount * multiplier;
  if (!Number.isSafeInteger(milliseconds) || milliseconds > 7 * 24 * 3_600_000) {
    throw new Error("duration must not exceed 7d");
  }
  return milliseconds;
}

function isMissingFile(error: unknown): boolean {
  return (
    error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === "ENOENT"
  );
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
