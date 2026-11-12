import { readFileSync } from "node:fs";
import { dirname, isAbsolute, resolve } from "node:path";
import { assertContainedPath } from "./snapshot.mjs";

export function sanitizeId(value) {
  const sanitized = String(value)
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9._-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  if (!sanitized) throw new Error(`Invalid empty identifier: ${value}`);
  return sanitized;
}

export function mulberry32(seed) {
  let state = seed >>> 0;
  return () => {
    state += 0x6d2b79f5;
    let value = state;
    value = Math.imul(value ^ (value >>> 15), value | 1);
    value ^= value + Math.imul(value ^ (value >>> 7), value | 61);
    return ((value ^ (value >>> 14)) >>> 0) / 4294967296;
  };
}

export function buildRunMatrix({ tasks, engines, cacheModes, repetitions, seed }) {
  const runs = [];
  const allCacheModes = [
    ...new Set([
      ...cacheModes,
      ...engines.flatMap((engine) => engine.cache_modes ?? []),
    ]),
  ];
  for (let repetition = 1; repetition <= repetitions; repetition += 1) {
    for (const task of tasks) {
      for (const cacheMode of allCacheModes) {
        for (const engine of engines) {
          const engineCacheModes = engine.cache_modes ?? cacheModes;
          if (!engineCacheModes.includes(cacheMode)) continue;
          runs.push({
            id: `${sanitizeId(task.id)}-${sanitizeId(cacheMode)}-${sanitizeId(engine.id)}-${repetition}`,
            task,
            engine,
            cacheMode,
            repetition,
          });
        }
      }
    }
  }
  const random = mulberry32(seed);
  for (let index = runs.length - 1; index > 0; index -= 1) {
    const other = Math.floor(random() * (index + 1));
    [runs[index], runs[other]] = [runs[other], runs[index]];
  }
  return runs;
}

export function selectRunEngines(config, engineId) {
  const engines = [...config.engines];
  if (!engineId) return engines;
  const selected = engines.filter((engine) => engine.id === engineId);
  if (selected.length === 0) throw new Error(`Unknown engine: ${engineId}`);
  return selected;
}

function walk(value, visit, path = []) {
  if (Array.isArray(value)) {
    value.forEach((item, index) => walk(item, visit, [...path, index]));
    return;
  }
  if (value && typeof value === "object") {
    visit(value, path);
    for (const [key, item] of Object.entries(value)) walk(item, visit, [...path, key]);
  }
}

export function emptyTelemetry() {
  return {
    events: 0,
    invalid_json_lines: 0,
    jsonl_bytes: 0,
    input_tokens: 0,
    cached_input_tokens: 0,
    output_tokens: 0,
    reasoning_output_tokens: 0,
    tool_calls: 0,
    mcp_calls: 0,
    mcp_calls_by_server: {},
    mcp_failures: 0,
    index_failures: 0,
    gcal_calls: 0,
    gcal_failures: 0,
    command_calls: 0,
    command_events: [],
    protocol_violations: [],
    event_types: {},
    waste: emptyWaste(),
  };
}

/**
 * Counts of what a run spent without getting an answer. Token totals of one
 * task differ by up to a factor of two between runs; these counts say why.
 */
export function emptyWaste() {
  return {
    /** Shell commands that exited non-zero. */
    failed_commands: 0,
    /** Of those, the ones that printed nothing (a quoting error swallows the whole line). */
    silent_failures: 0,
    /** Outputs that ended a list early and said so. */
    cut_outputs: 0,
    ambiguous_answers: 0,
    not_found_answers: 0,
    no_match_answers: 0,
    /** Edits written by hand. */
    patches: 0,
    gcal_replace_calls: 0,
    gcal_check_calls: 0,
    gcal_get_tests_calls: 0,
    /** Bytes of command output returned to the model. */
    output_bytes: 0,
  };
}

const count = (text, pattern) => text.match(pattern)?.length ?? 0;

/** Adds one completed Codex item (a shell command or a patch) to the waste counters. */
export function accumulateWaste(waste, event) {
  if (event?.type !== "item.completed") return waste;
  const item = event.item ?? {};
  if (item.type === "file_change") {
    waste.patches += 1;
    return waste;
  }
  if (item.type !== "command_execution") return waste;

  const command = Array.isArray(item.command) ? item.command.join(" ") : String(item.command ?? "");
  const output = String(item.aggregated_output ?? "");
  const exitCode = Number(item.exit_code);
  if (Number.isFinite(exitCode) && exitCode !== 0) {
    waste.failed_commands += 1;
    if (output.trim().length === 0) waste.silent_failures += 1;
  }
  waste.output_bytes += Buffer.byteLength(output);
  waste.cut_outputs += count(output, /^# \.\.\. \d+ more/gm);
  waste.ambiguous_answers += count(output, /^# ambiguous/gm);
  waste.not_found_answers += count(output, /^# not found/gm);
  waste.no_match_answers += count(output, /^# no (?:text )?match/gm);
  waste.gcal_replace_calls += count(command, /\back\s+replace\b/g);
  waste.gcal_check_calls += count(command, /\back\s+check\b/g);
  waste.gcal_get_tests_calls += count(command, /\back\s+get\b[^;&|]*--tests\b/g);
  return waste;
}

/** Waste counters plus the commands a run issued more than once. */
export function wasteSummary(telemetry) {
  const seen = new Set();
  let repeated = 0;
  for (const { command } of telemetry.command_events ?? []) {
    if (seen.has(command)) repeated += 1;
    seen.add(command);
  }
  return { ...(telemetry.waste ?? emptyWaste()), repeated_commands: repeated };
}

export function snapshotGcalEnvironment(environment) {
  return {
    ...environment,
    GCAL_DAEMON: "off",
  };
}

export function isGcalDaemonProcessCommand(
  commandLine,
  gcalHome,
  platform = process.platform,
) {
  const normalize = (value) => {
    const rendered = String(value ?? "").replaceAll("\\", "/");
    return platform === "win32" ? rendered.toLowerCase() : rendered;
  };
  const command = normalize(commandLine);
  const home = normalize(resolve(gcalHome));
  return (
    command.includes("daemonmain.js") &&
    command.includes("--gcal-home") &&
    command.includes(home)
  );
}

const SOURCE_FILE_PATTERN = /\.(?:java|ts|tsx|rs)(?:\b|["'`])/i;
const RAW_SEARCH_PATTERN =
  /(?:^|[\s"'`;&|])(?:rg|grep|findstr|select-string)(?:\.exe)?(?=\s|$)/i;
const NON_SOURCE_FILE_PATTERN =
  /\.(?:css|gradle|html|json|kts?|lock|md|properties|scss|toml|txt|xml|ya?ml)(?:\b|["'`])/i;
const DIRECT_READ_PATTERN =
  /(?:get-content|\bgc\b|\bcat\b|\btype\b|\bhead\b|\btail\b|\bmore\b|\bless\b|\bbat\b|\bsed\b|readfilesync|readalltext|git\s+(?:show|blame))/i;

const SHELL_WRAPPER_PATTERN = /\s-(?:Command|c)\s+(["'])([\s\S]*)\1\s*$/i;
const GCAL_INVOCATION_PATTERN = /^[\s"'`(]*gcal(?:\.(?:exe|cmd|ps1))?(?:\s|$)/i;
const GCAL_SUBCOMMAND_PATTERN = /\back(?:\.(?:exe|cmd|ps1))?\s+grep\b/gi;

// The script a `pwsh -Command "..."` wrapper runs, or the command itself.
function shellScript(command) {
  const wrapped = SHELL_WRAPPER_PATTERN.exec(command);
  if (wrapped === null) return command;
  return wrapped[1] === '"' ? wrapped[2].replace(/\\"/g, '"') : wrapped[2];
}

// Splits on shell separators outside quotes, so `'a|b'` in an argument stays whole.
function shellSegments(script) {
  const segments = [];
  let current = "";
  let quote = null;
  for (const char of script) {
    if (quote === null && (char === ";" || char === "&" || char === "|")) {
      segments.push(current);
      current = "";
      continue;
    }
    if (quote === char) quote = null;
    else if (quote === null && (char === '"' || char === "'")) quote = char;
    current += char;
  }
  segments.push(current);
  return segments;
}

export function isDirectSourceReadCommand(command) {
  const rendered = String(command ?? "");
  const commandHasSourceFile = SOURCE_FILE_PATTERN.test(rendered);
  const commandHasNonSourceFile = NON_SOURCE_FILE_PATTERN.test(rendered);
  for (const segment of shellSegments(shellScript(rendered))) {
    // `gcal grep Foo.java` is the GCAL surface, not a raw search or file read.
    if (GCAL_INVOCATION_PATTERN.test(segment)) continue;
    // Quotes nested inside a regex argument can split one `gcal grep` call in
    // the middle; the `grep` after `gcal` is still the GCAL subcommand.
    if (RAW_SEARCH_PATTERN.test(segment.replace(GCAL_SUBCOMMAND_PATTERN, " "))) {
      if (SOURCE_FILE_PATTERN.test(segment) || commandHasSourceFile) return true;
      if (!NON_SOURCE_FILE_PATTERN.test(segment) && !commandHasNonSourceFile) return true;
    }
    if (SOURCE_FILE_PATTERN.test(segment) && DIRECT_READ_PATTERN.test(segment)) return true;
  }
  return false;
}

export function protocolViolationsForEngine(violations, engineKind) {
  if (engineKind === "vanilla") {
    return violations.filter((violation) => violation.type !== "direct_source_read");
  }
  if (engineKind === "gcal") {
    return violations.filter((violation) => violation.type !== "gcal_cli");
  }
  // The hybrid lane may use gcal and shell tools alike.
  if (engineKind === "hybrid") {
    return violations.filter(
      (violation) => violation.type !== "gcal_cli" && violation.type !== "direct_source_read",
    );
  }
  return violations;
}

export function accumulateCodexLine(telemetry, line) {
  telemetry.jsonl_bytes += Buffer.byteLength(`${line}\n`);
  if (!line.trim()) return telemetry;
  let event;
  try {
    event = JSON.parse(line);
  } catch {
    telemetry.invalid_json_lines += 1;
    return telemetry;
  }
  telemetry.events += 1;
  const eventType = String(event.type ?? "unknown");
  telemetry.event_types[eventType] = (telemetry.event_types[eventType] ?? 0) + 1;
  accumulateWaste(telemetry.waste, event);

  const usageObjects = [];
  walk(event, (object, path) => {
    const last = path.at(-1);
    if (last === "usage" || last === "token_usage") usageObjects.push(object);
  });
  for (const usage of usageObjects) {
    telemetry.input_tokens = Math.max(
      telemetry.input_tokens,
      Number(usage.input_tokens ?? usage.prompt_tokens ?? 0),
    );
    telemetry.cached_input_tokens = Math.max(
      telemetry.cached_input_tokens,
      Number(usage.cached_input_tokens ?? usage.cache_read_input_tokens ?? 0),
    );
    telemetry.output_tokens = Math.max(
      telemetry.output_tokens,
      Number(usage.output_tokens ?? usage.completion_tokens ?? 0),
    );
    telemetry.reasoning_output_tokens = Math.max(
      telemetry.reasoning_output_tokens,
      Number(usage.reasoning_output_tokens ?? usage.reasoning_tokens ?? 0),
    );
  }

  const countToolCalls = !eventType.endsWith(".started");
  walk(event, (object) => {
    const kind = String(object.type ?? object.kind ?? "").toLowerCase();
    const name = String(
      object.name ?? object.tool ?? object.tool_name ?? object.server ?? "",
    ).toLowerCase();
    const server = String(object.server ?? object.mcp_server ?? "").toLowerCase();
    const inferredServer = name.match(/^mcp__(.+?)__/)?.[1] ?? "";
    const mcpServer = (server || inferredServer || (name.includes("codebase_memory") ? "codebase_memory_mcp" : ""))
      .replace(/[^a-z0-9_-]+/g, "_");
    if (
      countToolCalls &&
      (kind.includes("tool_call") || kind === "mcp_tool_call" || kind === "command_execution")
    ) {
      telemetry.tool_calls += 1;
      if (kind.includes("mcp") || name.includes("codebase_memory")) {
        telemetry.mcp_calls += 1;
        const key = mcpServer || "unknown";
        telemetry.mcp_calls_by_server[key] = (telemetry.mcp_calls_by_server[key] ?? 0) + 1;
      }
      if (kind.includes("command")) telemetry.command_calls += 1;
    }
    if (countToolCalls && kind === "mcp_tool_call" && object.status === "failed") {
      telemetry.mcp_failures += 1;
      if (name === "index_repository") telemetry.index_failures += 1;
    }
    const command = object.command ?? object.cmd;
    const rendered = Array.isArray(command) ? command.join(" ") : String(command ?? "");
    if (countToolCalls && rendered && (command !== undefined || kind.includes("command"))) {
      const rawExitCode = object.exit_code ?? object.exitCode;
      const exitCode = Number(rawExitCode);
      telemetry.command_events.push({
        command: rendered.slice(0, 2_000),
        exit_code: Number.isFinite(exitCode) ? exitCode : null,
      });
    }
    const isGcalCommand = /(?:^|[^A-Za-z0-9_.-])gcal(?:\.(?:exe|cmd|ps1))?(?:\s|$)/i.test(rendered);
    if (countToolCalls && isGcalCommand) {
      telemetry.gcal_calls += 1;
      const exitCode = object.exit_code ?? object.exitCode;
      if (object.status === "failed" || (Number.isFinite(exitCode) && exitCode !== 0)) {
        telemetry.gcal_failures += 1;
      }
      telemetry.protocol_violations.push({ type: "gcal_cli", command: rendered.slice(0, 500) });
    }
    if (countToolCalls && isDirectSourceReadCommand(rendered)) {
      telemetry.protocol_violations.push({
        type: "direct_source_read",
        command: rendered.slice(0, 500),
      });
    }
    const sourcePath = String(object.path ?? object.file_path ?? object.filename ?? "");
    if (
      countToolCalls &&
      SOURCE_FILE_PATTERN.test(sourcePath) &&
      (kind.includes("file_read") || name.includes("read_file"))
    ) {
      telemetry.protocol_violations.push({
        type: "direct_source_read",
        command: sourcePath.slice(0, 500),
      });
    }
  });
  return telemetry;
}

export function parseCodexJsonl(text) {
  const telemetry = emptyTelemetry();
  for (const line of text.split(/\r?\n/)) accumulateCodexLine(telemetry, line);
  return telemetry;
}

export function percentile(values, probability) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const index = Math.max(0, Math.ceil(probability * sorted.length) - 1);
  return sorted[index];
}

export function median(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[middle - 1] + sorted[middle]) / 2
    : sorted[middle];
}

export function sampleStandardDeviation(values) {
  if (values.length < 2) return null;
  const valueMean = mean(values);
  const variance = values.reduce((sum, value) => sum + ((value - valueMean) ** 2), 0) /
    (values.length - 1);
  return Math.sqrt(variance);
}

export function coefficientOfVariation(values) {
  if (values.length < 2) return null;
  const valueMean = mean(values);
  if (valueMean === 0) return null;
  return sampleStandardDeviation(values) / valueMean;
}

export function summarizeRuns(runs) {
  const enriched = runs.map((run) => ({
    ...run,
    uncached_input_tokens: run.input_tokens - run.cached_input_tokens,
    uncached_plus_output_tokens:
      run.input_tokens - run.cached_input_tokens + run.output_tokens,
  }));
  const groups = new Map();
  for (const run of enriched) {
    const key = `${run.task_id}\u0000${run.cache_mode}\u0000${run.engine}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(run);
  }
  return [...groups.values()].map((group) => {
    const successful = group.filter((run) => run.success);
    const metric = (name) => successful.map((run) => run[name]).filter(Number.isFinite);
    const summary = {
      task_id: group[0].task_id,
      cache_mode: group[0].cache_mode,
      engine: group[0].engine,
      runs: group.length,
      successes: successful.length,
      success_rate: successful.length / group.length,
      successful_wall_ms_p95: percentile(metric("wall_ms"), 0.95),
    };
    for (const name of SUMMARY_METRICS) {
      Object.assign(summary, successfulMetricSummary(name, metric(name)));
    }
    return summary;
  });
}

const SUMMARY_METRICS = [
  "wall_ms",
  "verified_e2e_ms",
  "input_tokens",
  "cached_input_tokens",
  "uncached_input_tokens",
  "output_tokens",
  "uncached_plus_output_tokens",
  "total_tokens",
  "setup_ms",
  "preindex_ms",
  "completion_ms",
  "mcp_calls",
  "gcal_calls",
  "gcal_failures",
  "cache_bytes",
  "patch_bytes",
];

function successfulMetricSummary(name, values) {
  return {
    [`successful_${name}_mean`]: mean(values),
    [`successful_${name}_median`]: median(values),
    [`successful_${name}_p50`]: median(values),
    [`successful_${name}_range`]: range(values),
    [`successful_${name}_sample_sd`]: sampleStandardDeviation(values),
    [`successful_${name}_cv`]: coefficientOfVariation(values),
  };
}

function mean(values) {
  if (values.length === 0) return null;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function range(values) {
  if (values.length === 0) return null;
  return Math.max(...values) - Math.min(...values);
}

export function tomlString(value) {
  return JSON.stringify(String(value));
}

export function tomlInlineTable(object) {
  return `{ ${Object.entries(object)
    .map(([key, value]) => `${JSON.stringify(key)} = ${tomlString(value)}`)
    .join(", ")} }`;
}

// `codeModeHost: false` passes `features.code_mode_host=false`. It worked around
// an unstable nested host in mid-2026 Codex builds, but from Codex 0.159 the
// host is the agent's only way to execute commands: disabling it leaves the
// model with no tools ("Code Mode is unavailable because code-mode host is
// disabled"). The Codex default is therefore left alone unless a config asks.
export function codexSandboxArgs({ fullAccess, worktree, codeModeHost }) {
  const hostArgs = codeModeHost === false ? ["-c", "features.code_mode_host=false"] : [];
  if (fullAccess) {
    return ["-s", "danger-full-access", "-c", 'approval_policy="never"', ...hostArgs];
  }
  return [
    "-s",
    "workspace-write",
    "--add-dir",
    worktree,
    "-c",
    'approval_policy="never"',
    ...hostArgs,
  ];
}

export function expandTokens(value, tokens) {
  if (Array.isArray(value)) return value.map((item) => expandTokens(item, tokens));
  if (value && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, expandTokens(item, tokens)]));
  }
  if (typeof value !== "string") return value;
  return value.replace(/\{([a-zA-Z0-9_]+)\}/g, (match, name) => {
    if (!(name in tokens)) throw new Error(`Unknown placeholder ${match}`);
    return String(tokens[name]);
  });
}

function resolveRelative(base, value) {
  if (!value || isAbsolute(value)) return value;
  if (!value.startsWith(".") && !value.includes("/") && !value.includes("\\")) return value;
  return resolve(base, value);
}

export function resolveFromConfig(configPath, value) {
  return resolveRelative(dirname(resolve(configPath)), value);
}

export function normalizeReadySnapshot(config, configPath) {
  const ready = config.ready_snapshot;
  if (!ready) return undefined;
  for (const field of [
    "root",
    "worktree",
    "live_cache",
    "allowed_worktree_root",
    "allowed_cache_root",
    "allowed_snapshot_root",
  ]) {
    if (typeof ready[field] !== "string" || !ready[field]) {
      throw new Error(`ready_snapshot.${field} is required`);
    }
  }
  const normalized = {
    root: resolveFromConfig(configPath, ready.root),
    worktree: resolve(ready.worktree),
    live_cache: resolve(ready.live_cache),
    allowed_worktree_root: resolve(ready.allowed_worktree_root),
    allowed_cache_root: resolve(ready.allowed_cache_root),
    allowed_snapshot_root: resolveFromConfig(configPath, ready.allowed_snapshot_root),
  };
  normalized.root = assertContainedPath(
    normalized.root,
    normalized.allowed_snapshot_root,
    "snapshot",
  );
  normalized.worktree = assertContainedPath(
    normalized.worktree,
    normalized.allowed_worktree_root,
    "worktree",
  );
  normalized.live_cache = assertContainedPath(
    normalized.live_cache,
    normalized.allowed_cache_root,
    "live cache",
  );
  if (config.repo && normalized.worktree === resolve(config.repo)) {
    throw new Error("worktree must not equal source repository");
  }
  if (normalized.root === normalized.live_cache) {
    throw new Error("snapshot and live cache must be distinct");
  }
  return normalized;
}

export function resolveRunLayout({ kind, readySnapshot, runId, worktreeRoot, cacheRoot }) {
  if (kind === "gcal" && readySnapshot) {
    return {
      worktree: readySnapshot.worktree,
      cacheDir: readySnapshot.live_cache,
      usesReadySnapshot: true,
    };
  }
  return {
    worktree: resolve(worktreeRoot, runId),
    cacheDir: resolve(cacheRoot, runId),
    usesReadySnapshot: false,
  };
}

export function shouldPrimeIndex({ kind, usesReadySnapshot }) {
  return kind === "gcal" && !usesReadySnapshot;
}

export function resolveRepositoryGate({ sourceRepository, worktree, usesReadySnapshot }) {
  return usesReadySnapshot ? worktree : sourceRepository;
}

export function loadConfig(configPath) {
  const absolutePath = resolve(configPath);
  const base = dirname(absolutePath);
  const config = JSON.parse(readFileSync(absolutePath, "utf8"));
  if (!Array.isArray(config.tasks) || config.tasks.length === 0) throw new Error("Config needs tasks");
  if (!Array.isArray(config.engines) || config.engines.length === 0) {
    throw new Error("Config needs at least one engine");
  }
  const ids = new Set();
  for (const item of [...config.tasks, ...config.engines]) {
    if (!item.id) throw new Error("Every task and engine needs an id");
    const id = sanitizeId(item.id);
    if (ids.has(id)) throw new Error(`Duplicate id: ${id}`);
    ids.add(id);
  }
  config.repo = isAbsolute(config.repo) ? config.repo : resolve(base, config.repo);
  config.ready_snapshot = normalizeReadySnapshot(config, absolutePath);
  config.output = resolveRelative(base, config.output);
  for (const task of config.tasks) {
    task.prompt_file = resolveRelative(base, task.prompt_file);
    task.grader.command = resolveRelative(base, task.grader.command);
    task.grader.args = (task.grader.args ?? []).map((arg) => resolveRelative(base, arg));
    if (task.overlay !== undefined) {
      if (typeof task.overlay?.patch_file !== "string" || task.overlay.patch_file.length === 0) {
        throw new Error(`Task ${task.id} overlay needs a patch_file`);
      }
      task.overlay.patch_file = resolveRelative(base, task.overlay.patch_file);
    }
  }
  for (const engine of config.engines) {
    engine.kind ??= "mcp";
    if (!["gcal", "mcp", "serena", "vanilla"].includes(engine.kind)) {
      throw new Error(`Unknown engine kind for ${engine.id}: ${engine.kind}`);
    }
    if (engine.kind !== "vanilla" && !engine.command) {
      throw new Error(`Engine ${engine.id} needs a command`);
    }
    engine.command = resolveRelative(base, engine.command);
    if (engine.kind === "gcal") {
      if (!engine.backend_command) throw new Error(`GCAL engine ${engine.id} needs backend_command`);
      engine.backend_command = resolveRelative(base, engine.backend_command);
    }
  }
  return { config, path: absolutePath };
}
