import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { closeSync, mkdirSync, openSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const STALE_LOCK_MS = 10_000;

export function daemonEndpoint(gcalHome: string): string {
  if (process.platform === "win32") {
    const identity = createHash("sha256")
      .update(resolve(gcalHome).toLowerCase())
      .digest("hex")
      .slice(0, 20);
    return `\\\\.\\pipe\\gcal-${identity}`;
  }

  return join(resolve(gcalHome), "daemon.sock");
}

export function daemonLockPath(gcalHome: string): string {
  return join(resolve(gcalHome), "daemon-start.lock");
}

export interface DetachedDaemonOptions {
  gcalHome: string;
  endpoint: string;
  idleTimeoutMs: number;
  entryPath?: string;
  spawnProcess?: typeof spawn;
}

export function startDetachedDaemon(options: DetachedDaemonOptions): void {
  mkdirSync(options.gcalHome, { recursive: true });
  const lockPath = daemonLockPath(options.gcalHome);
  if (!acquireLock(lockPath)) return;

  const entryPath =
    options.entryPath ?? fileURLToPath(new URL("../../daemonMain.js", import.meta.url));

  try {
    const child = (options.spawnProcess ?? spawn)(
      process.execPath,
      [
        entryPath,
        "--endpoint",
        options.endpoint,
        "--gcal-home",
        options.gcalHome,
        "--idle-ms",
        String(options.idleTimeoutMs),
      ],
      {
        cwd: tmpdir(),
        detached: true,
        stdio: "ignore",
        windowsHide: true,
      },
    );
    child.unref();
  } catch (error) {
    rmSync(lockPath, { force: true });
    throw error;
  }
}

function acquireLock(lockPath: string): boolean {
  try {
    closeSync(openSync(lockPath, "wx"));
    return true;
  } catch (error) {
    if (!isAlreadyExists(error)) throw error;
  }

  try {
    if (Date.now() - statSync(lockPath).mtimeMs <= STALE_LOCK_MS) return false;
    rmSync(lockPath, { force: true });
    closeSync(openSync(lockPath, "wx"));
    return true;
  } catch (error) {
    if (isAlreadyExists(error)) return false;
    throw error;
  }
}

function isAlreadyExists(error: unknown): boolean {
  return (
    error instanceof Error && "code" in error && (error as NodeJS.ErrnoException).code === "EEXIST"
  );
}
