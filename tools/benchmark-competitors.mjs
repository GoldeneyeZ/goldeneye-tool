import { spawn, spawnSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";

const workspace = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const suffix = process.platform === "win32" ? ".exe" : "";
const QUERY_CASE_NAMES = Object.freeze([
  "exact_search",
  "bm25_search",
  "code_search",
  "trace",
  "snippet",
  "architecture",
  "cypher_node",
  "cypher_qualified_name",
]);

const flags = new Map();
for (let index = 2; index < process.argv.length; index += 1) {
  const name = process.argv[index];
  const value = process.argv[index + 1];
  if (value !== undefined && !value.startsWith("--")) {
    flags.set(name, value);
    index += 1;
  } else {
    flags.set(name, true);
  }
}

if (flags.has("--help")) {
  console.log(`Usage:
  node tools/benchmark-competitors.mjs --repo <path> [options]

Options:
  --goldeneye <path>       default: target/release/goldeneye${suffix}
  --comparator <path>      default: codebase-memory-mcp from PATH
  --out <path>             default: target/benchmarks/latest.json
  --cold-runs <n>          default: 1
  --warmups <n>            default: 3
  --samples <n>            default: 20
  --startup-wait-ms <n>    default: 6000
  --goldeneye-response-mode <text|dual>
                            default: text
  --cases <names>          comma-separated query cases; default: all
  --skip-build             skip cargo release build
  --keep-temp              keep isolated cache directories`);
  process.exit(0);
}

const requiredRepo = flags.get("--repo");
if (typeof requiredRepo !== "string") {
  fail("--repo is required");
}

const config = {
  repo: resolve(requiredRepo),
  goldeneye: resolve(
    String(
      flags.get("--goldeneye") ??
        join(workspace, "target", "release", `goldeneye${suffix}`),
    ),
  ),
  comparator: String(flags.get("--comparator") ?? "codebase-memory-mcp"),
  out: resolve(
    String(flags.get("--out") ?? join(workspace, "target", "benchmarks", "latest.json")),
  ),
  coldRuns: integerFlag("--cold-runs", 1),
  warmups: integerFlag("--warmups", 3),
  samples: integerFlag("--samples", 20),
  startupWaitMs: integerFlag("--startup-wait-ms", 6000, true),
  goldeneyeResponseMode: choiceFlag("--goldeneye-response-mode", "text", ["text", "dual"]),
  selectedCases: caseFlag(),
  skipBuild: flags.has("--skip-build"),
  keepTemp: flags.has("--keep-temp"),
};

if (!existsSync(config.repo) || !statSync(config.repo).isDirectory()) {
  fail(`repository does not exist: ${config.repo}`);
}

class Mcp {
  constructor(command, environment) {
    this.id = 0;
    this.pending = new Map();
    this.stderr = "";
    this.child = spawn(command, [], {
      cwd: workspace,
      env: environment,
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });
    this.child.stderr.on("data", chunk => {
      this.stderr += chunk.toString();
    });
    this.child.on("error", error => this.rejectAll(error));
    createInterface({ input: this.child.stdout }).on("line", line => {
      let message;
      try {
        message = JSON.parse(line);
      } catch (error) {
        this.rejectAll(new Error(`non-JSON stdout: ${line}: ${error.message}`));
        return;
      }
      const pending = this.pending.get(message.id);
      if (pending === undefined) return;
      this.pending.delete(message.id);
      if (message.error !== undefined) {
        pending.reject(new Error(JSON.stringify(message.error)));
        return;
      }
      pending.resolve({
        result: message.result,
        ms: performance.now() - pending.started,
        wireBytes: Buffer.byteLength(line),
      });
    });
  }

  request(method, params) {
    const id = ++this.id;
    return new Promise((resolvePromise, reject) => {
      this.pending.set(id, { resolve: resolvePromise, reject, started: performance.now() });
      this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    });
  }

  notify(method, params) {
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
  }

  async initialize() {
    await this.request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "goldeneye-benchmark", version: "1" },
    });
    this.notify("notifications/initialized", {});
    if (config.startupWaitMs > 0) await sleep(config.startupWaitMs);
  }

  async tools() {
    return (await this.request("tools/list", {})).result.tools;
  }

  async call(name, args) {
    const response = await this.request("tools/call", { name, arguments: args });
    if (response.result?.isError) throw new Error(`${name}: ${JSON.stringify(response.result)}`);
    const text = response.result?.content?.find(item => item.type === "text")?.text;
    if (text === undefined) throw new Error(`${name}: missing text response`);
    let payload;
    try {
      payload = JSON.parse(text);
    } catch {
      payload = { text };
    }
    return {
      payload,
      ms: response.ms,
      contentBytes: Buffer.byteLength(text),
      structuredContentBytes:
        response.result?.structuredContent === undefined
          ? 0
          : Buffer.byteLength(JSON.stringify(response.result.structuredContent)),
      representationCount: response.result?.structuredContent === undefined ? 1 : 2,
      wireBytes: response.wireBytes,
    };
  }

  async close() {
    const closed = new Promise(resolvePromise => this.child.once("close", resolvePromise));
    this.child.stdin.end();
    await closed;
  }

  rejectAll(error) {
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
  }
}

async function start(engine, root) {
  const environment = {
    ...process.env,
    CBM_ALLOWED_ROOT: dirname(config.repo),
    CBM_CACHE_DIR: root,
    CBM_SEMANTIC_ENABLED: "1",
    CBM_SEMANTIC_THRESHOLD: "0.82",
    GOLDENEYE_PROJECT_ROOT: config.repo,
  };
  if (engine.name === "goldeneye") {
    environment.GOLDENEYE_DB_PATH = join(root, "goldeneye.db");
    environment.GOLDENEYE_MCP_RESPONSE_MODE = config.goldeneyeResponseMode;
  } else {
    delete environment.GOLDENEYE_DB_PATH;
    delete environment.GOLDENEYE_MCP_RESPONSE_MODE;
  }
  const session = new Mcp(engine.command, environment);
  await session.initialize();
  return session;
}

async function benchmarkEngine(engine) {
  const indexSamples = [];
  let active;
  for (let run = 0; run < config.coldRuns; run += 1) {
    if (active !== undefined) await cleanup(active);
    const root = mkdtempSync(join(tmpdir(), `goldeneye-bench-${engine.name}-`));
    const session = await start(engine, root);
    const indexed = await session.call("index_repository", {
      repo_path: config.repo,
      mode: "full",
      persistence: false,
    });
    indexSamples.push(indexed.ms);
    active = { root, session, indexed };
  }

  try {
    const project = active.indexed.payload.project;
    const tools = new Map((await active.session.tools()).map(tool => [tool.name, tool]));
    const exact = await active.session.call("search_graph", {
      project,
      name_pattern: "^fs_search$",
      limit: 20,
    });
    const qualifiedName = exact.payload.results?.[0]?.qualified_name;
    if (qualifiedName === undefined) throw new Error(`${engine.name}: fs_search not found`);

    const searchCodeSchema = tools.get("search_code")?.inputSchema?.properties ?? {};
    const searchCode = { project, limit: 20 };
    if (searchCodeSchema.pattern !== undefined) searchCode.pattern = "node_modules";
    else if (searchCodeSchema.query !== undefined) searchCode.query = "node_modules";
    else throw new Error(`${engine.name}: unknown search_code schema`);
    if (searchCodeSchema.mode !== undefined) searchCode.mode = "compact";
    if (searchCodeSchema.regex !== undefined) searchCode.regex = false;

    const cypher = { project };
    if (tools.get("query_graph")?.inputSchema?.properties?.max_rows !== undefined) {
      cypher.max_rows = 200;
    }
    const cases = {
      exact_search: ["search_graph", { project, name_pattern: "^fs_search$", limit: 20 }],
      bm25_search: ["search_graph", { project, query: "fs search", limit: 20 }],
      code_search: ["search_code", searchCode],
      trace: [
        "trace_path",
        {
          project,
          function_name: "fs_search",
          direction: "outbound",
          depth: 1,
          mode: "calls",
          edge_types: ["CALLS"],
        },
      ],
      snippet: ["get_code_snippet", { project, qualified_name: qualifiedName }],
      architecture: ["get_architecture", { project }],
      cypher_node: [
        "query_graph",
        { ...cypher, query: "MATCH (n) RETURN n LIMIT 20" },
      ],
      cypher_qualified_name: [
        "query_graph",
        { ...cypher, query: "MATCH (n) RETURN n.qualified_name LIMIT 20" },
      ],
    };

    const queries = {};
    for (const [name, [tool, args]] of Object.entries(cases)) {
      if (config.selectedCases !== null && !config.selectedCases.has(name)) {
        continue;
      }
      const request = { tool, arguments: args };
      try {
        queries[name] = { request, ...(await measure(active.session, tool, args)) };
      } catch (error) {
        queries[name] = { request, error: error.message };
      }
    }
    return {
      command: engine.command,
      version: version(engine.command),
      index: {
        ...statistics(indexSamples),
        cache_bytes: directoryBytes(active.root),
        files: active.indexed.payload.files,
        nodes: active.indexed.payload.nodes,
        edges: active.indexed.payload.edges,
        response: responseSummary(
          active.indexed.payload,
          active.indexed.contentBytes,
          active.indexed.structuredContentBytes,
          active.indexed.representationCount,
          active.indexed.wireBytes,
        ),
      },
      queries,
    };
  } finally {
    await cleanup(active);
  }
}

async function measure(session, tool, args) {
  for (let index = 0; index < config.warmups; index += 1) await session.call(tool, args);
  const samples = [];
  let last;
  for (let index = 0; index < config.samples; index += 1) {
    last = await session.call(tool, args);
    samples.push(last.ms);
  }
  return {
    ...statistics(samples),
    response: responseSummary(
      last.payload,
      last.contentBytes,
      last.structuredContentBytes,
      last.representationCount,
      last.wireBytes,
    ),
  };
}

function responseSummary(
  payload,
  contentBytes,
  structuredContentBytes,
  representationCount,
  wireBytes,
) {
  const result = {
    content_bytes: contentBytes,
    structured_content_bytes: structuredContentBytes,
    representation_count: representationCount,
    wire_bytes: wireBytes,
  };
  if (Number.isFinite(payload.total)) result.total = payload.total;
  if (Array.isArray(payload.rows)) {
    result.returned = payload.rows.length;
    if (Array.isArray(payload.rows[0])) {
      result.row_shape = payload.rows[0].map(value =>
        value === null ? "null" : Array.isArray(value) ? "array" : typeof value,
      );
    }
  } else if (Array.isArray(payload.results)) result.returned = payload.results.length;
  else if (Array.isArray(payload.paths)) result.returned = payload.paths.length;
  else if (Array.isArray(payload.hops)) result.returned = payload.hops.length;
  else if (Array.isArray(payload.callers) || Array.isArray(payload.callees)) {
    result.returned = (payload.callers?.length ?? 0) + (payload.callees?.length ?? 0);
  }
  const hopKeys = ["paths", "hops", "callers", "callees"].filter(key =>
    Array.isArray(payload[key]),
  );
  if (hopKeys.length > 0) {
    result.serialized_hop_instances = hopKeys.reduce(
      (count, key) => count + payload[key].length,
      0,
    );
  }
  if (typeof payload.truncated === "boolean") result.truncated = payload.truncated;
  for (const key of ["types", "modules", "entry_points", "file_tree"]) {
    if (Array.isArray(payload[key])) result[`${key}_count`] = payload[key].length;
  }
  return result;
}

function statistics(samples) {
  const sorted = [...samples].sort((left, right) => left - right);
  const rank = percentile => sorted[Math.max(0, Math.ceil(percentile * sorted.length) - 1)];
  return {
    samples_ms: samples.map(round),
    min_ms: round(sorted[0]),
    p50_ms: round(rank(0.5)),
    p95_ms: round(rank(0.95)),
    max_ms: round(sorted.at(-1)),
  };
}

function comparisons(goldeneye, comparator) {
  const result = {};
  for (const name of Object.keys(goldeneye.queries)) {
    const goldeneyeQuery = goldeneye.queries[name];
    const comparatorQuery = comparator.queries[name];
    if (!Number.isFinite(goldeneyeQuery?.p50_ms) || !Number.isFinite(comparatorQuery?.p50_ms)) {
      continue;
    }
    result[name] = {
      goldeneye_p50_ms: goldeneyeQuery.p50_ms,
      codebase_memory_p50_ms: comparatorQuery.p50_ms,
      goldeneye_p95_ms: goldeneyeQuery.p95_ms,
      codebase_memory_p95_ms: comparatorQuery.p95_ms,
      ratio: round(goldeneyeQuery.p50_ms / comparatorQuery.p50_ms),
      p95_ratio: round(goldeneyeQuery.p95_ms / comparatorQuery.p95_ms),
      goldeneye_request: goldeneyeQuery.request,
      codebase_memory_request: comparatorQuery.request,
      goldeneye_response: goldeneyeQuery.response,
      codebase_memory_response: comparatorQuery.response,
    };
  }
  return result;
}

function version(command) {
  const result = spawnSync(command, ["--version"], { encoding: "utf8", windowsHide: true });
  return result.status === 0 ? result.stdout.trim() : `unknown: ${result.stderr.trim()}`;
}

function directoryBytes(root) {
  let total = 0;
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const path = join(root, entry.name);
    total += entry.isDirectory() ? directoryBytes(path) : entry.isFile() ? statSync(path).size : 0;
  }
  return total;
}

async function cleanup(state) {
  await state.session.close();
  if (!config.keepTemp) rmSync(state.root, { recursive: true, force: true });
}

function integerFlag(name, fallback, allowZero = false) {
  const value = Number.parseInt(String(flags.get(name) ?? fallback), 10);
  if (!Number.isSafeInteger(value) || value < (allowZero ? 0 : 1)) fail(`${name}: invalid integer`);
  return value;
}

function choiceFlag(name, fallback, choices) {
  const value = String(flags.get(name) ?? fallback);
  if (!choices.includes(value)) fail(`${name}: expected one of ${choices.join(", ")}`);
  return value;
}

function caseFlag() {
  const raw = flags.get("--cases");
  if (raw === undefined) return null;
  if (typeof raw !== "string") {
    fail("--cases requires a comma-separated value");
  }

  const names = raw.split(",").map((name) => name.trim());
  if (names.length === 0 || names.some((name) => name.length === 0)) {
    fail("--cases: names must be non-empty");
  }

  const unknown = names.filter((name) => !QUERY_CASE_NAMES.includes(name));
  if (unknown.length > 0) {
    fail(
      `--cases: unknown case(s): ${unknown.join(", ")}; expected one of ${QUERY_CASE_NAMES.join(", ")}`,
    );
  }

  return new Set(names);
}

function round(value) {
  return Math.round(value * 1000) / 1000;
}

function sleep(ms) {
  return new Promise(resolvePromise => setTimeout(resolvePromise, ms));
}

function fail(message) {
  console.error(`Competitor benchmark failed: ${message}`);
  process.exit(1);
}

try {
  if (!config.skipBuild) {
    const build = spawnSync("cargo", ["build", "--release", "-p", "goldeneye"], {
      cwd: workspace,
      stdio: "inherit",
      windowsHide: true,
    });
    if (build.status !== 0) fail("cargo build failed");
  }

  const comparator = await benchmarkEngine({
    name: "codebase-memory-mcp",
    command: config.comparator,
  });
  const goldeneye = await benchmarkEngine({ name: "goldeneye", command: config.goldeneye });
  const result = {
    generated_at: new Date().toISOString(),
    repository: config.repo,
    settings: {
      mode: "full",
      cold_runs: config.coldRuns,
      warmups: config.warmups,
      samples: config.samples,
      startup_wait_ms: config.startupWaitMs,
      goldeneye_response_mode: config.goldeneyeResponseMode,
      cases: QUERY_CASE_NAMES.filter(
        (name) => config.selectedCases === null || config.selectedCases.has(name),
      ),
    },
    engines: { "codebase-memory-mcp": comparator, goldeneye },
    comparisons: comparisons(goldeneye, comparator),
  };
  mkdirSync(dirname(config.out), { recursive: true });
  writeFileSync(config.out, `${JSON.stringify(result, null, 2)}\n`);
  console.log(`Benchmark artifact: ${config.out}`);
  for (const [name, comparison] of Object.entries(result.comparisons)) {
    console.log(
      `${name.padEnd(24)} Goldeneye=${comparison.goldeneye_p50_ms.toFixed(3)}ms comparator=${comparison.codebase_memory_p50_ms.toFixed(3)}ms ratio=${comparison.ratio.toFixed(3)}x`,
    );
  }
} catch (error) {
  fail(error instanceof Error ? error.message : String(error));
}
