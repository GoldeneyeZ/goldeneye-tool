import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  DEFAULT_DAEMON_IDLE_TIMEOUT_MS,
  parseIdleTimeout,
  resolveDaemonConfig,
} from "../src/config/daemonConfig.js";

const temporaryDirectories: string[] = [];

afterEach(async () => {
  await Promise.all(
    temporaryDirectories
      .splice(0)
      .map((directory) => rm(directory, { recursive: true, force: true })),
  );
});

describe("daemon config", () => {
  it("defaults to an automatic daemon with ten-minute idle expiry", async () => {
    const gcalHome = await temporaryDirectory();

    expect(resolveDaemonConfig(gcalHome, {})).toEqual({
      config: { mode: "auto", idleTimeoutMs: DEFAULT_DAEMON_IDLE_TIMEOUT_MS },
      warnings: [],
    });
  });

  it("loads file config and applies environment overrides", async () => {
    const gcalHome = await temporaryDirectory();
    await writeFile(
      join(gcalHome, "config.json"),
      JSON.stringify({ daemon: { mode: "off", idleTimeout: "2m" } }),
    );

    expect(
      resolveDaemonConfig(gcalHome, {
        GCAL_DAEMON: "auto",
        GCAL_DAEMON_IDLE: "30s",
      }),
    ).toEqual({
      config: { mode: "auto", idleTimeoutMs: 30_000 },
      warnings: [],
    });
  });

  it("keeps usable defaults and reports invalid configuration", async () => {
    const gcalHome = await temporaryDirectory();
    await writeFile(join(gcalHome, "config.json"), "{broken");

    const resolved = resolveDaemonConfig(gcalHome, {
      GCAL_DAEMON: "sometimes",
      GCAL_DAEMON_IDLE: "ten minutes",
    });

    expect(resolved.config).toEqual({
      mode: "auto",
      idleTimeoutMs: DEFAULT_DAEMON_IDLE_TIMEOUT_MS,
    });
    expect(resolved.warnings).toHaveLength(3);
  });

  it.each([
    ["500ms", 500],
    ["30s", 30_000],
    ["10m", 600_000],
    ["2h", 7_200_000],
  ])("parses %s", (value, expected) => {
    expect(parseIdleTimeout(value)).toBe(expected);
  });
});

async function temporaryDirectory(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "gcal-daemon-config-"));
  temporaryDirectories.push(directory);
  return directory;
}
