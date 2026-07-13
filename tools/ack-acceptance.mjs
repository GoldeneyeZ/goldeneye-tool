#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const workspace = resolve(scriptDirectory, "..");
const fixture = join(workspace, "tests", "fixtures", "ack", "rust-project");
const expected = JSON.parse(
  readFileSync(join(workspace, "tests", "fixtures", "ack", "expected.json"), "utf8"),
);
const argumentsMap = parseArguments(process.argv.slice(2));
const ackRoot = resolve(
  argumentsMap.get("--ack-root") ?? process.env.ACK_ROOT ?? fail("--ack-root or ACK_ROOT is required"),
);
const ackMain = join(ackRoot, "dist", "main.js");
const server = resolve(
  argumentsMap.get("--goldeneye-bin") ??
    join(workspace, "target", "debug", `goldeneye${process.platform === "win32" ? ".exe" : ""}`),
);
const temporaryRoot = mkdtempSync(join(tmpdir(), "goldeneye-ack-acceptance-"));
const temporaryProject = join(temporaryRoot, "fixture");
const database = join(temporaryRoot, "state", "goldeneye.db");
const results = [];

try {
  assert.ok(existsSync(ackMain), `ACK dist entry point does not exist: ${ackMain}`);
  assert.ok(existsSync(fixture), `ACK fixture does not exist: ${fixture}`);

  if (!argumentsMap.has("--goldeneye-bin")) {
    const build = spawnSync("cargo", ["build", "--quiet", "-p", "goldeneye"], {
      cwd: workspace,
      encoding: "utf8",
      windowsHide: true,
    });
    assert.equal(build.status, 0, commandFailure("cargo build", build));
  }
  assert.ok(existsSync(server), `Goldeneye binary does not exist: ${server}`);

  cpSync(fixture, temporaryProject, { recursive: true });
  const isolatedEnvironment = {
    ...process.env,
    ACK_MCP_COMMAND: server,
    CBM_ALLOWED_ROOT: temporaryRoot,
    GOLDENEYE_DB_PATH: database,
    GOLDENEYE_PROJECT_ROOT: temporaryRoot,
    PATH: "",
  };
  delete isolatedEnvironment.ACK_MCP_URL;
  delete isolatedEnvironment.ACK_PROJECT;

  const version = run(server, ["--version"], isolatedEnvironment);
  assert.equal(version.status, 0, commandFailure("goldeneye --version", version));
  assert.match(version.stdout.trim(), /^goldeneye \d+\.\d+\.\d+$/);
  results.push(result("server identity", version));

  const indexed = runAck("index", ["index", temporaryProject], isolatedEnvironment);
  assert.equal(indexed.status, 0, commandFailure("ack index", indexed));
  const indexPayload = JSON.parse(indexed.stdout);
  const project = indexPayload.project;
  const canonicalRoot = indexPayload.root_path;
  assert.equal(typeof project, "string");
  assert.ok(project.length > 0);
  assert.deepEqual(normalizeValue(indexPayload, project, canonicalRoot), expected.index);
  assert.ok(existsSync(database), "Rust server did not create isolated database");
  assert.ok(statSync(database).size > 0, "isolated database is empty");

  const projectEnvironment = { ...isolatedEnvironment, ACK_PROJECT: project };
  const qualifiedHelper = `${project}.src.lib.helper`;
  const qualifiedEntry = `${project}.src.lib.entry`;

  expectJson("status", ["status"], expected.status, projectEnvironment, project, canonicalRoot);
  expectText(
    "search",
    ["search", "helper", "--limit", "5"],
    expected.search,
    projectEnvironment,
    project,
  );
  expectText(
    "symbol",
    ["symbol", ".*duplicate.*", "--limit", "5"],
    expected.symbol,
    projectEnvironment,
    project,
  );
  expectText(
    "inspect",
    ["inspect", qualifiedHelper],
    expected.inspect,
    projectEnvironment,
    project,
  );
  expectText("get exact", ["get", qualifiedHelper], expected.source, projectEnvironment, project);
  expectText("get suffix", ["get", "src.lib.helper"], expected.source, projectEnvironment, project);
  expectText("get short", ["get", "helper"], expected.source, projectEnvironment, project);
  expectText(
    "callers",
    ["callers", qualifiedHelper, "--depth", "1", "--limit", "20"],
    expected.callers,
    projectEnvironment,
    project,
  );
  expectText(
    "callees",
    ["callees", qualifiedEntry, "--depth", "1", "--limit", "20"],
    expected.callees,
    projectEnvironment,
    project,
  );
  expectJson(
    "arch",
    ["arch"],
    expected.architecture,
    projectEnvironment,
    project,
    canonicalRoot,
  );
  expectError(
    "ambiguity",
    ["get", "duplicate"],
    expected.ambiguity_error,
    projectEnvironment,
    project,
  );
  expectError(
    "suggestions",
    ["get", "helpe"],
    expected.suggestion_error,
    projectEnvironment,
    project,
  );
  expectError(
    "missing project",
    ["status"],
    expected.missing_project_error,
    { ...isolatedEnvironment, ACK_PROJECT: "missing-project" },
    project,
  );

  const unselected = runAck("unselected project", ["status"], isolatedEnvironment);
  assert.equal(unselected.status, 2, commandFailure("ack status without project", unselected));
  assert.equal(unselected.stderr.trimEnd(), expected.unselected_project_error);

  const help = runAck("help", ["--help"], isolatedEnvironment);
  assert.equal(help.status, 0, commandFailure("ack --help", help));
  assert.doesNotMatch(help.stdout, /\belect\b/);
  assert.match(help.stdout, /\bstatus\b/);
  assert.match(help.stdout, /\barch\b/);

  for (const item of results) {
    console.log(`PASS ${item.name} exit=${item.exitCode} bytes=${item.bytes}`);
  }
  console.log(`PASS Rust-only isolation project=${project} database_bytes=${statSync(database).size}`);
} catch (error) {
  console.error(`ACK acceptance failed: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
} finally {
  rmSync(temporaryRoot, { force: true, recursive: true });
}

function parseArguments(values) {
  const parsed = new Map();
  for (let index = 0; index < values.length; index += 1) {
    const name = values[index];
    if (name !== "--ack-root" && name !== "--goldeneye-bin") {
      fail(`unknown argument: ${name}`);
    }
    const value = values[index + 1];
    if (value === undefined) fail(`missing value for ${name}`);
    parsed.set(name, value);
    index += 1;
  }
  return parsed;
}

function fail(message) {
  throw new Error(message);
}

function run(executable, args, environment) {
  const completed = spawnSync(executable, args, {
    cwd: workspace,
    encoding: "utf8",
    env: environment,
    maxBuffer: 1024 * 1024,
    windowsHide: true,
  });
  return {
    status: completed.status,
    stdout: completed.stdout ?? "",
    stderr: completed.stderr ?? "",
    error: completed.error,
  };
}

function runAck(name, args, environment) {
  const completed = run(process.execPath, [ackMain, ...args], environment);
  results.push(result(`ack ${name}`, completed));
  return completed;
}

function result(name, completed) {
  return {
    name,
    exitCode: completed.status ?? "spawn-error",
    bytes: Buffer.byteLength(completed.stdout) + Buffer.byteLength(completed.stderr),
  };
}

function expectText(name, args, wanted, environment, project) {
  const completed = runAck(name, args, environment);
  assert.equal(completed.status, 0, commandFailure(`ack ${name}`, completed));
  assert.equal(normalizeText(completed.stdout.trimEnd(), project), wanted);
  assert.equal(completed.stderr, "");
  assert.ok(Buffer.byteLength(completed.stdout) <= 4096, `${name} output is not compact`);
}

function expectJson(name, args, wanted, environment, project, canonicalRoot) {
  const completed = runAck(name, args, environment);
  assert.equal(completed.status, 0, commandFailure(`ack ${name}`, completed));
  assert.equal(completed.stderr, "");
  assert.deepEqual(
    normalizeValue(JSON.parse(completed.stdout), project, canonicalRoot),
    wanted,
  );
  assert.ok(Buffer.byteLength(completed.stdout) <= 4096, `${name} output is not compact`);
}

function expectError(name, args, wanted, environment, project) {
  const completed = runAck(name, args, environment);
  assert.equal(completed.status, 1, commandFailure(`ack ${name}`, completed));
  assert.equal(completed.stdout, "");
  assert.equal(normalizeText(completed.stderr.trimEnd(), project), wanted);
  assert.ok(Buffer.byteLength(completed.stderr) <= 4096, `${name} error is not compact`);
}

function normalizeText(text, project) {
  return text.split(project).join("<project>").replaceAll("\r\n", "\n");
}

function normalizeValue(value, project, canonicalRoot) {
  if (typeof value === "string") {
    return normalizeText(value, project).split(canonicalRoot).join("<root>");
  }
  if (Array.isArray(value)) {
    return value.map((item) => normalizeValue(item, project, canonicalRoot));
  }
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(
      Object.entries(value).map(([key, item]) => [key, normalizeValue(item, project, canonicalRoot)]),
    );
  }
  return value;
}

function commandFailure(name, completed) {
  if (completed.error) return `${name} spawn failed: ${completed.error.message}`;
  const detail = completed.stderr.trim() || completed.stdout.trim();
  return `${name} exited ${String(completed.status)}${detail ? `: ${detail}` : ""}`;
}
