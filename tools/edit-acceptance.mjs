#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import {
  cpSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const scriptDirectory = dirname(fileURLToPath(import.meta.url));
const workspace = resolve(scriptDirectory, "..");
const fixture = join(workspace, "tests", "fixtures", "edit", "rust-project");
const expectedTools = JSON.parse(
  readFileSync(join(workspace, "tests", "fixtures", "edit", "expected-tools.json"), "utf8"),
);
const argumentsMap = parseArguments(process.argv.slice(2));
const server = resolve(
  argumentsMap.get("--goldeneye-bin") ??
    join(workspace, "target", "debug", `goldeneye${process.platform === "win32" ? ".exe" : ""}`),
);
const temporaryRoot = mkdtempSync(join(tmpdir(), "goldeneye-edit-acceptance-"));
const projectRoot = join(temporaryRoot, "fixture");
const outsideRoot = join(temporaryRoot, "outside");
const database = join(temporaryRoot, "state", "goldeneye.db");
const results = [];

try {
  assert.ok(existsSync(fixture), `edit fixture does not exist: ${fixture}`);
  if (!argumentsMap.has("--goldeneye-bin")) {
    const build = run("cargo", ["build", "--quiet", "-p", "goldeneye"]);
    assert.equal(build.status, 0, commandFailure("cargo build", build));
  }
  assert.ok(existsSync(server), `Goldeneye binary does not exist: ${server}`);
  cpSync(fixture, projectRoot, { recursive: true });
  cpSync(fixture, outsideRoot, { recursive: true });

  const environment = {
    ...process.env,
    CBM_ALLOWED_ROOT: temporaryRoot,
    GOLDENEYE_DB_PATH: database,
    GOLDENEYE_PROJECT_ROOT: temporaryRoot,
  };
  let session = await McpSession.open(server, environment);
  const tools = await session.request("tools/list", {});
  assert.deepEqual(
    tools.tools.map((tool) => tool.name),
    expectedTools,
  );
  assert.ok(tools.tools.every((tool) => tool.inputSchema.type === "object"));
  assert.ok(tools.tools.every((tool) => tool.outputSchema.type === "object"));
  assert.ok(!tools.tools.some((tool) => tool.name === "elect"));
  pass("registry", JSON.stringify(tools).length);

  const indexed = await session.callSuccess("index_repository", {
    repo_path: projectRoot,
    mode: "fast",
  });
  const project = indexed.project;
  assert.equal(indexed.status, "indexed");
  const first = await inspect(session, project);
  const staleHelper = locator(first, "fn helper");
  const replaced = await session.callSuccess("replace_node", {
    operation_id: "accept-replace",
    locator: staleHelper,
    content: "pub fn helper() -> usize { 2 }",
    parse_policy: "require_clean",
  });
  assert.ok(replaced.changed_syntax_ids.length > 0);
  assert.ok(replaced.changed_graph_ids.length > 0);
  assert.ok(replaced.size.approximate_context_tokens > 0);

  const afterReplaceBytes = readFileSync(join(projectRoot, "src", "lib.rs"));
  const stale = await session.callError("replace_node", {
    operation_id: "accept-stale",
    locator: staleHelper,
    content: "pub fn helper() -> usize { 99 }",
  });
  assert.match(stale, /conflict:.*fresh_syntax=/s);
  assert.deepEqual(readFileSync(join(projectRoot, "src", "lib.rs")), afterReplaceBytes);

  let current = await inspect(session, project);
  await session.callSuccess("insert_before_node", {
    operation_id: "accept-before",
    locator: locator(current, "fn helper"),
    content: "pub fn injected() -> usize { 9 }\n",
  });
  current = await inspect(session, project);
  await session.callSuccess("delete_node", {
    operation_id: "accept-delete",
    locator: locator(current, "fn injected"),
  });
  current = await inspect(session, project);
  const inserted = await session.callSuccess("insert_after_node", {
    operation_id: "accept-after",
    locator: locator(current, "fn helper"),
    content: "\npub fn after_helper() -> usize { 8 }",
  });
  const created = await session.callSuccess("create_file", {
    operation_id: "accept-create",
    project,
    path: "src/nested/extra.rs",
    content: "pub fn extra() -> usize { 3 }\n",
    expected_generation: inserted.generation,
    parse_policy: "require_clean",
    create_parents: true,
  });
  assert.ok(existsSync(join(projectRoot, "src", "nested", "extra.rs")));

  const existing = await session.callError("create_file", {
    operation_id: "accept-existing",
    project,
    path: "src/nested/extra.rs",
    content: "overwrite",
    expected_generation: created.generation,
    create_parents: true,
  });
  assert.match(existing, /conflict:.*already exists/s);
  const escaped = await session.callError("create_file", {
    operation_id: "accept-escape",
    project,
    path: "../escape.rs",
    content: "escape",
    expected_generation: created.generation,
  });
  assert.match(escaped, /Invalid parameters|forbidden|invalid_input/);
  assert.ok(!existsSync(join(temporaryRoot, "escape.rs")));

  const link = join(projectRoot, "src", "outside-link");
  symlinkSync(outsideRoot, link, process.platform === "win32" ? "junction" : "dir");
  const symlinkEscape = await session.callError("create_file", {
    operation_id: "accept-symlink",
    project,
    path: "src/outside-link/evil.rs",
    content: "pub fn evil() {}\n",
    expected_generation: created.generation,
    create_parents: true,
  });
  assert.match(symlinkEscape, /forbidden:|escapes project/);
  assert.ok(!existsSync(join(outsideRoot, "evil.rs")));

  const helper = await uniqueFunction(session, project, "helper");
  const helperSource = await session.callSuccess("get_code_snippet", {
    project,
    qualified_name: helper.qualified_name,
  });
  assert.match(helperSource.source, /helper\(\).*\{ 2 \}/s);
  const callers = await session.callSuccess("trace_path", {
    project,
    function_name: helper.qualified_name,
    direction: "inbound",
    depth: 1,
    mode: "calls",
  });
  assert.ok(callers.callers.length > 0);
  const extra = await uniqueFunction(session, project, "extra");
  const extraSource = await session.callSuccess("get_code_snippet", {
    project,
    qualified_name: extra.qualified_name,
  });
  assert.match(extraSource.source, /fn extra/);
  pass("mutations and immediate ACK reads", JSON.stringify({ replaced, created }).length);

  await session.close();
  session = await McpSession.open(server, environment);
  const restarted = await inspect(session, project);
  assert.ok(restarted.syntax.n.some((node) => node.v?.includes("fn helper")));
  assert.ok((await uniqueFunction(session, project, "extra")).qualified_name.includes("extra"));
  await session.close();
  pass("clean restart", JSON.stringify(restarted).length);

  const recovery = run("cargo", [
    "test",
    "--quiet",
    "-p",
    "goldeneye",
    "--test",
    "stdio_services",
    "stdio_startup_recovers_interrupted_edit_before_first_response",
  ]);
  assert.equal(recovery.status, 0, commandFailure("restart recovery test", recovery));
  pass("interrupted restart recovery", recovery.stdout.length + recovery.stderr.length);

  for (const item of results) {
    console.log(`PASS ${item.name} bytes=${item.bytes}`);
  }
  console.log(`PASS edit acceptance project=${project}`);
} catch (error) {
  console.error(`Edit acceptance failed: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
} finally {
  rmSync(temporaryRoot, { force: true, recursive: true });
}

async function inspect(session, project) {
  return session.callSuccess("inspect_syntax", {
    project,
    path: "src/lib.rs",
    inspect: {
      max_depth: 8,
      max_nodes: 200,
      preview_chars: 128,
      node_kinds: [],
    },
  });
}

function locator(inspection, preview) {
  const index = inspection.syntax.n.findIndex(
    (node) => node.k === "function_item" && node.v?.includes(preview),
  );
  assert.notEqual(index, -1, `syntax node not found: ${preview}`);
  return inspection.locators[index];
}

async function uniqueFunction(session, project, name) {
  const search = await session.callSuccess("search_graph", {
    project,
    name_pattern: `^${name}$`,
    limit: 20,
  });
  const functions = search.results.filter((node) => node.label === "Function");
  assert.equal(functions.length, 1, `expected one Function named ${name}`);
  return functions[0];
}

class McpSession {
  static async open(executable, environment) {
    const session = new McpSession(executable, environment);
    await session.request("ping", {});
    return session;
  }

  constructor(executable, environment) {
    this.nextId = 1;
    this.pending = new Map();
    this.stderr = "";
    this.child = spawn(executable, [], {
      cwd: workspace,
      env: environment,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    createInterface({ input: this.child.stdout }).on("line", (line) => {
      let response;
      try {
        response = JSON.parse(line);
      } catch (error) {
        this.rejectAll(new Error(`non-JSON stdout: ${line}: ${error.message}`));
        return;
      }
      const pending = this.pending.get(response.id);
      if (pending === undefined) return;
      clearTimeout(pending.timeout);
      this.pending.delete(response.id);
      if (response.error !== undefined) pending.reject(new Error(JSON.stringify(response.error)));
      else pending.resolve(response.result);
    });
    this.child.stderr.setEncoding("utf8");
    this.child.stderr.on("data", (chunk) => {
      this.stderr += chunk;
    });
    this.child.on("error", (error) => this.rejectAll(error));
    this.exit = new Promise((resolveExit) => {
      this.child.on("close", (code) => {
        if (this.pending.size > 0) {
          this.rejectAll(new Error(`server exited ${code}: ${this.stderr.trim()}`));
        }
        resolveExit(code);
      });
    });
  }

  request(method, params) {
    const id = this.nextId;
    this.nextId += 1;
    return new Promise((resolveRequest, rejectRequest) => {
      const timeout = setTimeout(() => {
        this.pending.delete(id);
        rejectRequest(new Error(`request timed out: ${method}`));
      }, 15_000);
      this.pending.set(id, { resolve: resolveRequest, reject: rejectRequest, timeout });
      this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }

  async callSuccess(name, argumentsValue) {
    const result = await this.request("tools/call", { name, arguments: argumentsValue });
    assert.equal(result.isError, false, `${name} failed: ${JSON.stringify(result)}`);
    assert.deepEqual(JSON.parse(result.content[0].text), result.structuredContent);
    return result.structuredContent;
  }

  async callError(name, argumentsValue) {
    const result = await this.request("tools/call", { name, arguments: argumentsValue });
    assert.equal(result.isError, true, `${name} unexpectedly succeeded`);
    assert.equal(result.structuredContent, undefined);
    return result.content[0].text;
  }

  async close() {
    this.child.stdin.end();
    const code = await this.exit;
    assert.equal(code, 0, `server exited ${code}: ${this.stderr.trim()}`);
    assert.equal(this.stderr, "", `server stderr: ${this.stderr}`);
  }

  rejectAll(error) {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(error);
    }
    this.pending.clear();
  }
}

function parseArguments(values) {
  const parsed = new Map();
  for (let index = 0; index < values.length; index += 1) {
    const name = values[index];
    if (name !== "--goldeneye-bin") fail(`unknown argument: ${name}`);
    const value = values[index + 1];
    if (value === undefined) fail(`missing value for ${name}`);
    parsed.set(name, value);
    index += 1;
  }
  return parsed;
}

function run(executable, args) {
  const completed = spawnSync(executable, args, {
    cwd: workspace,
    encoding: "utf8",
    env: process.env,
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

function pass(name, bytes) {
  results.push({ name, bytes });
}

function fail(message) {
  throw new Error(message);
}

function commandFailure(name, completed) {
  if (completed.error) return `${name} spawn failed: ${completed.error.message}`;
  const detail = completed.stderr.trim() || completed.stdout.trim();
  return `${name} exited ${String(completed.status)}${detail ? `: ${detail}` : ""}`;
}
