import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  buildRunMatrix,
  codexSandboxArgs,
  isGcalDaemonProcessCommand,
  isDirectSourceReadCommand,
  loadConfig,
  parseCodexJsonl,
  protocolViolationsForEngine,
  resolveRepositoryGate,
  resolveRunLayout,
  sanitizeId,
  selectRunEngines,
  shouldPrimeIndex,
  snapshotGcalEnvironment,
  summarizeRuns,
  tomlInlineTable,
} from "./core.mjs";

const BENCH_ROOT = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = resolve(BENCH_ROOT, "../..");
const AGENT_RUNNER = join(BENCH_ROOT, "bin", "benchmark-agent-tasks.mjs");

test("codexSandboxArgs grants full access with the tool host enabled", () => {
  assert.deepEqual(codexSandboxArgs({ fullAccess: true, worktree: "D:\\repo" }), [
    "-s",
    "danger-full-access",
    "-c",
    'approval_policy="never"',
    "-c",
    "features.code_mode_host=true",
  ]);
});

test("sanitizeId creates stable filesystem-safe identifiers", () => {
  assert.equal(sanitizeId("Terax / Fuzzy Diacritics"), "terax-fuzzy-diacritics");
  assert.throws(() => sanitizeId("///"), /empty identifier/);
});

test("buildRunMatrix is complete and deterministic", () => {
  const input = {
    tasks: [{ id: "task-a" }, { id: "task-b" }],
    engines: [{ id: "goldeneye" }, { id: "cbm" }],
    cacheModes: ["cold", "warm"],
    repetitions: 2,
    seed: 42,
  };
  const first = buildRunMatrix(input).map((run) => run.id);
  const second = buildRunMatrix(input).map((run) => run.id);
  assert.deepEqual(first, second);
  assert.equal(first.length, 16);
  assert.equal(new Set(first).size, 16);
});

test("buildRunMatrix runs vanilla once without cold/warm duplication", () => {
  const runs = buildRunMatrix({
    tasks: [{ id: "task" }],
    engines: [
      { id: "goldeneye" },
      { id: "cbm" },
      { id: "vanilla", kind: "vanilla", cache_modes: ["none"] },
    ],
    cacheModes: ["cold", "warm"],
    repetitions: 2,
    seed: 1,
  });
  assert.equal(runs.length, 10);
  const vanilla = runs.filter((run) => run.engine.id === "vanilla");
  assert.equal(vanilla.length, 2);
  assert.deepEqual(new Set(vanilla.map((run) => run.cacheMode)), new Set(["none"]));
});

test("selectRunEngines preserves the GCAL provenance engine when selecting vanilla", () => {
  const gcal = { id: "goldeneye-code-agent-layer", kind: "gcal" };
  const vanilla = { id: "vanilla", kind: "vanilla" };
  const config = { engines: [gcal, vanilla] };

  assert.deepEqual(selectRunEngines(config, "vanilla"), [vanilla]);
  assert.deepEqual(config.engines, [gcal, vanilla]);
  assert.equal(config.engines.find((engine) => engine.kind === "gcal"), gcal);
});

test("calibration mode validates its vanilla-only single-run contract before repository access", () => {
  const directory = mkdtempSync(join(tmpdir(), "agent-bench-calibration-"));
  const configPath = join(directory, "config.json");
  writeFileSync(configPath, JSON.stringify({
    repo: join(directory, "missing-repository"),
    output: join(directory, "report.json"),
    repetitions: 2,
    tasks: [{ id: "task", prompt_file: "task.md", grader: { command: "grader.mjs" } }],
    engines: [
      { id: "gcal", kind: "gcal", command: "gcal", backend_command: "goldeneye" },
      { id: "vanilla", kind: "vanilla" },
    ],
  }));
  try {
    const run = (args) => spawnSync(
      process.execPath,
      [AGENT_RUNNER, "--config", configPath, "--calibration", ...args],
      { cwd: REPO_ROOT, encoding: "utf8" },
    );
    assert.match(run(["--calibration-id", "attempt-1"]).stderr, /requires --engine <vanilla-id>/);
    assert.match(
      run(["--engine", "vanilla", "--calibration-id", "attempt-1"]).stderr,
      /requires --repetitions 1/,
    );
    assert.match(
      run(["--engine", "vanilla", "--repetitions", "1"]).stderr,
      /--calibration-id is required/,
    );
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

test("parseCodexJsonl extracts cumulative usage, tool calls, bytes, and violations", () => {
  const lines = [
    JSON.stringify({ type: "thread.started", thread_id: "abc" }),
    JSON.stringify({
      type: "item.completed",
      item: { type: "mcp_tool_call", server: "codebase_memory_mcp", name: "search_graph" },
    }),
    JSON.stringify({
      type: "item.completed",
      item: { type: "command_execution", command: "gcal search Foo" },
    }),
    JSON.stringify({
      type: "turn.completed",
      usage: {
        input_tokens: 1200,
        cached_input_tokens: 300,
        output_tokens: 250,
        reasoning_output_tokens: 40,
      },
    }),
    JSON.stringify({
      type: "item.completed",
      item: {
        type: "mcp_tool_call",
        server: "codebase_memory_mcp",
        tool: "index_repository",
        status: "failed",
      },
    }),
    "not json",
  ];
  const telemetry = parseCodexJsonl(lines.join("\n"));
  assert.equal(telemetry.events, 5);
  assert.equal(telemetry.invalid_json_lines, 1);
  assert.equal(telemetry.input_tokens, 1200);
  assert.equal(telemetry.cached_input_tokens, 300);
  assert.equal(telemetry.output_tokens, 250);
  assert.equal(telemetry.reasoning_output_tokens, 40);
  assert.equal(telemetry.tool_calls, 3);
  assert.equal(telemetry.mcp_calls, 2);
  assert.deepEqual(telemetry.mcp_calls_by_server, { codebase_memory_mcp: 2 });
  assert.equal(telemetry.mcp_failures, 1);
  assert.equal(telemetry.index_failures, 1);
  assert.equal(telemetry.gcal_calls, 1);
  assert.equal(telemetry.gcal_failures, 0);
  assert.equal(telemetry.command_calls, 1);
  assert.equal(telemetry.protocol_violations.length, 1);
  assert.ok(telemetry.jsonl_bytes > 0);
});

test("direct Java, TypeScript, and Rust reads are protocol violations for MCP lanes", () => {
  assert.equal(isDirectSourceReadCommand("Get-Content src/main/java/App.java"), true);
  assert.equal(isDirectSourceReadCommand("Get-Content src/app/App.tsx"), true);
  assert.equal(isDirectSourceReadCommand("cat src-tauri/src/main.rs"), true);
  assert.equal(isDirectSourceReadCommand("rg -n 'fuzzyScore' src"), true);
  assert.equal(
    isDirectSourceReadCommand("Select-String -Path src-tauri/Cargo.toml -Pattern name"),
    false,
  );
  assert.equal(
    isDirectSourceReadCommand("Get-Content package.json | Select-String -Pattern scripts"),
    false,
  );
  assert.equal(isDirectSourceReadCommand(`rg "spring-boot-starter-test|junit" pom.xml`), false);
  assert.equal(isDirectSourceReadCommand("pnpm vitest run src/app/App.test.tsx"), false);
  assert.equal(isDirectSourceReadCommand("git diff -- src/app/App.tsx"), false);
  assert.equal(
    isDirectSourceReadCommand(
      "Get-Content tsconfig.json -Raw; git diff -- src/app/App.tsx; git status --short",
    ),
    false,
  );

  const telemetry = parseCodexJsonl(
    JSON.stringify({
      type: "item.completed",
      item: { type: "command_execution", command: "Get-Content src/app/App.tsx" },
    }),
  );
  assert.deepEqual(telemetry.protocol_violations.map((item) => item.type), [
    "direct_source_read",
  ]);
  assert.equal(protocolViolationsForEngine(telemetry.protocol_violations, "mcp").length, 1);
  assert.equal(protocolViolationsForEngine(telemetry.protocol_violations, "vanilla").length, 0);
});

test("GCAL lanes allow GCAL commands while preserving direct source-read violations", () => {
  const telemetry = parseCodexJsonl(
    [
      {
        type: "item.completed",
        item: {
          type: "command_execution",
          command: `gcal workflow --js "return await gcal.search('SecurityConfig')"`,
          exit_code: 0,
        },
      },
      {
        type: "item.completed",
        item: { type: "command_execution", command: "gcal get Missing", exit_code: 1 },
      },
      {
        type: "item.completed",
        item: { type: "command_execution", command: "Get-Content src/main/java/App.java" },
      },
    ]
      .map(JSON.stringify)
      .join("\n"),
  );
  assert.equal(telemetry.gcal_calls, 2);
  assert.equal(telemetry.gcal_failures, 1);
  assert.deepEqual(
    protocolViolationsForEngine(telemetry.protocol_violations, "gcal").map((item) => item.type),
    ["direct_source_read"],
  );
});

test("snapshot GCAL initialization disables daemon reuse without dropping engine env", () => {
  assert.deepEqual(
    snapshotGcalEnvironment({
      GCAL_BACKEND: "goldeneye",
      GCAL_DAEMON: "on",
      GCAL_DAEMON_IDLE: "10m",
      GOLDENEYE_DB_PATH: "snapshot.db",
    }),
    {
      GCAL_BACKEND: "goldeneye",
      GCAL_DAEMON: "off",
      GCAL_DAEMON_IDLE: "10m",
      GOLDENEYE_DB_PATH: "snapshot.db",
    },
  );
});

test("GCAL daemon process matching is scoped to the exact GCAL home", () => {
  const gcalHome = "D:\\Dev\\IdeaProjects\\.gab-cache\\ssrb5-live\\gcal-state";
  const matching =
    `"C:\\nvm4w\\nodejs\\node.exe" C:\\gcal\\dist\\daemonMain.js ` +
    `--endpoint \\\\.\\pipe\\gcal-123 --gcal-home ${gcalHome} --idle-ms 600000`;

  assert.equal(isGcalDaemonProcessCommand(matching, gcalHome, "win32"), true);
  assert.equal(
    isGcalDaemonProcessCommand(
      matching,
      "D:\\Dev\\IdeaProjects\\.gab-cache\\other\\gcal-state",
      "win32",
    ),
    false,
  );
  assert.equal(isGcalDaemonProcessCommand("goldeneye mcp --stdio", gcalHome, "win32"), false);
});

test("summarizeRuns never rewards failed runs with fast timings", () => {
  const summary = summarizeRuns([
    {
      task_id: "task",
      cache_mode: "cold",
      engine: "goldeneye",
      success: false,
      wall_ms: 1,
      input_tokens: 1,
      output_tokens: 1,
      total_tokens: 2,
      patch_bytes: 0,
    },
    {
      task_id: "task",
      cache_mode: "cold",
      engine: "goldeneye",
      success: true,
      wall_ms: 100,
      setup_ms: 2,
      preindex_ms: 10,
      completion_ms: 112,
      verified_e2e_ms: 130,
      input_tokens: 50,
      output_tokens: 10,
      total_tokens: 60,
      patch_bytes: 200,
    },
  ])[0];
  assert.equal(summary.success_rate, 0.5);
  assert.equal(summary.successful_wall_ms_p50, 100);
  assert.equal(summary.successful_completion_ms_p50, 112);
  assert.equal(summary.successful_verified_e2e_ms_p50, 130);
  assert.equal(summary.successful_total_tokens_p50, 60);
});

test("summarizeRuns reports sample SD and CV", () => {
  const summary = summarizeRuns([
    {
      task_id: "task",
      cache_mode: "cold",
      engine: "goldeneye",
      success: true,
      wall_ms: 100,
      input_tokens: 800,
      cached_input_tokens: 600,
      output_tokens: 20,
      total_tokens: 820,
    },
    {
      task_id: "task",
      cache_mode: "cold",
      engine: "goldeneye",
      success: true,
      wall_ms: 200,
      input_tokens: 1000,
      cached_input_tokens: 700,
      output_tokens: 30,
      total_tokens: 1030,
    },
    {
      task_id: "task",
      cache_mode: "cold",
      engine: "goldeneye",
      success: true,
      wall_ms: 300,
      input_tokens: 1200,
      cached_input_tokens: 800,
      output_tokens: 40,
      total_tokens: 1240,
    },
  ])[0];
  assert.equal(summary.successful_wall_ms_mean, 200);
  assert.equal(summary.successful_wall_ms_sample_sd, 100);
  assert.equal(summary.successful_wall_ms_cv, 0.5);
  assert.equal(summary.successful_uncached_input_tokens_p50, 300);
  assert.equal(summary.successful_uncached_plus_output_tokens_p50, 330);
});

test("tomlInlineTable quotes Windows paths and environment keys", () => {
  assert.equal(
    tomlInlineTable({ CBM_CACHE_DIR: "D:\\cache path" }),
    '{ "CBM_CACHE_DIR" = "D:\\\\cache path" }',
  );
});

test("loadConfig normalizes and validates ready snapshot paths", () => {
  const directory = BENCH_ROOT;
  const configPath = join(directory, `ready-snapshot-${process.pid}.json`);
  const config = {
    repo: "../spring-framework",
    output: "out/report.json",
    tasks: [{ id: "task", prompt_file: "task.md", grader: { command: "grader.mjs" } }],
    engines: [{ id: "gcal", kind: "gcal", command: "gcal", backend_command: "goldeneye" }],
    ready_snapshot: {
      root: "../../target/agent-bench/snapshots/spring-stringutils",
      worktree: "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
      live_cache: "D:\\Dev\\IdeaProjects\\.gab-cache\\spring-stringutils-live",
      allowed_worktree_root: "D:\\Dev\\IdeaProjects\\.gab",
      allowed_cache_root: "D:\\Dev\\IdeaProjects\\.gab-cache",
      allowed_snapshot_root: "../../target/agent-bench/snapshots",
    },
  };
  try {
    writeFileSync(configPath, JSON.stringify(config));
    const { config: normalized } = loadConfig(configPath);
    assert.equal(normalized.ready_snapshot.root, resolve(directory, "../../target/agent-bench/snapshots/spring-stringutils"));
    assert.equal(normalized.ready_snapshot.worktree, resolve(config.ready_snapshot.worktree));
    assert.equal(normalized.ready_snapshot.live_cache, resolve(config.ready_snapshot.live_cache));
    assert.equal(
      normalized.ready_snapshot.allowed_snapshot_root,
      resolve(directory, config.ready_snapshot.allowed_snapshot_root),
    );

    delete config.ready_snapshot.allowed_snapshot_root;
    writeFileSync(configPath, JSON.stringify(config));
    assert.throws(() => loadConfig(configPath), /ready_snapshot\.allowed_snapshot_root/);

    config.ready_snapshot.allowed_snapshot_root = "../../target/agent-bench/snapshots";
    config.ready_snapshot.worktree = config.ready_snapshot.allowed_worktree_root;
    writeFileSync(configPath, JSON.stringify(config));
    assert.throws(() => loadConfig(configPath), /worktree must be a strict descendant/);

    config.repo = "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree";
    config.ready_snapshot.worktree = config.repo;
    writeFileSync(configPath, JSON.stringify(config));
    assert.throws(() => loadConfig(configPath), /worktree must not equal source repository/);

    config.ready_snapshot.worktree = "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree";
    config.ready_snapshot.live_cache = config.ready_snapshot.allowed_cache_root;
    writeFileSync(configPath, JSON.stringify(config));
    assert.throws(() => loadConfig(configPath), /live cache must be a strict descendant/);
  } finally {
    rmSync(configPath, { force: true });
  }
});

test("Level-2 Spring config declares the million-token qualification and audit policy", () => {
  const config = JSON.parse(readFileSync(join(
    BENCH_ROOT,
    "configs",
    "spring-sensitive-value-redaction-level2.json",
  ), "utf8"));

  assert.deepEqual(config.qualification, {
    min_input_tokens: 800_000,
    max_input_tokens: 1_200_000,
    min_uncached_input_tokens: 100_000,
  });
  assert.deepEqual(config.audit, {
    expected_candidate_runs: 3,
    expected_vanilla_runs: 3,
  });
  assert.equal(config.allowed_dirty_policy.max_paths, 40);
  assert.equal(config.tasks[0].source_language, "Java");
  assert.deepEqual(config.tasks[0].source_extensions, [".java"]);
});

test("ready snapshots use stable GCAL paths and skip priming", () => {
  const readySnapshot = {
    root: "D:\\Dev\\IdeaProjects\\goldeneye-tool\\target\\agent-bench\\snapshots\\spring-stringutils",
    worktree: "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
    live_cache: "D:\\Dev\\IdeaProjects\\.gab-cache\\spring-stringutils-live",
  };
  assert.deepEqual(
    resolveRunLayout({ kind: "gcal", readySnapshot, runId: "candidate-1" }),
    {
      worktree: readySnapshot.worktree,
      cacheDir: readySnapshot.live_cache,
      usesReadySnapshot: true,
    },
  );
  assert.equal(shouldPrimeIndex({ kind: "gcal", usesReadySnapshot: true }), false);

  const vanilla = resolveRunLayout({
    kind: "vanilla",
    readySnapshot,
    runId: "vanilla-1",
    worktreeRoot: "D:\\runs\\worktrees",
    cacheRoot: "D:\\runs\\cache",
  });
  assert.deepEqual(vanilla, {
    worktree: resolve("D:\\runs\\worktrees", "vanilla-1"),
    cacheDir: resolve("D:\\runs\\cache", "vanilla-1"),
    usesReadySnapshot: false,
  });
  assert.equal(shouldPrimeIndex({ kind: "vanilla", usesReadySnapshot: false }), false);

  assert.equal(
    resolveRepositoryGate({
      sourceRepository: "D:\\Dev\\IdeaProjects\\spring-framework",
      worktree: readySnapshot.worktree,
      usesReadySnapshot: true,
    }),
    readySnapshot.worktree,
  );
});

test("prepare-snapshot exits before spawning Codex", () => {
  const directory = mkdtempSync(join(tmpdir(), "agent-bench-prepare-"));
  const repo = join(directory, "source");
  const fakeGcal = join(repo, "dist", "main.js");
  const fakeBackend = join(repo, "dist", "backend.mjs");
  const configPath = join(directory, "config.json");
  const codexMarker = join(directory, "codex-spawned");
  const recoveryEvidence = {
    fixture: {
      path: join(directory, "recovery.json"),
      sha256: "a".repeat(64),
    },
  };
  const ready = {
    root: join(directory, "snapshots", "ready"),
    worktree: join(directory, "worktrees", "stable"),
    live_cache: join(directory, "cache", "live"),
    allowed_worktree_root: join(directory, "worktrees"),
    allowed_cache_root: join(directory, "cache"),
    allowed_snapshot_root: join(directory, "snapshots"),
  };
  try {
    spawnSync("git", ["init", repo], { encoding: "utf8" });
    writeFileSync(join(repo, "README.md"), "ready\n");
    writeFileSync(join(repo, "package-lock.json"), "{\"lockfileVersion\":3}\n");
    mkdirSync(join(repo, "dist"), { recursive: true });
    writeFileSync(
      fakeGcal,
      `import { spawn } from "node:child_process";\nimport { mkdirSync, writeFileSync } from "node:fs";\nimport { dirname, join } from "node:path";\nimport { fileURLToPath } from "node:url";\nmkdirSync(process.env.GCAL_HOME, { recursive: true });\nwriteFileSync(join(process.env.GCAL_HOME, "projects.json"), "{}");\nmkdirSync(dirname(process.env.GOLDENEYE_DB_PATH), { recursive: true });\nconst child = spawn(process.execPath, [fileURLToPath(new URL("./backend.mjs", import.meta.url))], { detached: true, env: process.env, stdio: "ignore", windowsHide: true });\nchild.unref();\nawait new Promise((resolveDelay) => setTimeout(resolveDelay, 1_500));\n`,
    );
    writeFileSync(
      fakeBackend,
      `import { DatabaseSync } from "node:sqlite";\nconst database = new DatabaseSync(process.env.GOLDENEYE_DB_PATH);\ndatabase.exec("PRAGMA journal_mode = WAL; CREATE TABLE evidence(value TEXT NOT NULL); INSERT INTO evidence VALUES ('ready')");\nawait new Promise((resolveDelay) => setTimeout(resolveDelay, 2_500));\ndatabase.exec("INSERT INTO evidence VALUES ('closed')");\ndatabase.close();\n`,
    );
    spawnSync("git", ["-C", repo, "add", "README.md"], { encoding: "utf8" });
    spawnSync("git", ["-C", repo, "add", "package-lock.json"], { encoding: "utf8" });
    spawnSync("git", ["-C", repo, "add", "dist/main.js"], { encoding: "utf8" });
    spawnSync("git", ["-C", repo, "add", "dist/backend.mjs"], { encoding: "utf8" });
    spawnSync(
      "git",
      ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "init"],
      { encoding: "utf8" },
    );
    writeFileSync(
      configPath,
      JSON.stringify({
        repo,
        output: join(directory, "report.json"),
        codex_command: codexMarker,
        tasks: [{ id: "task", prompt_file: join(repo, "README.md"), grader: { command: process.execPath } }],
        engines: [{
          id: "gcal",
          kind: "gcal",
          command: process.execPath,
          args: [fakeGcal],
          backend_command: process.execPath,
        }],
        ready_snapshot: ready,
        recovery_evidence: recoveryEvidence,
      }),
    );
    mkdirSync(ready.allowed_worktree_root, { recursive: true });
    spawnSync("git", ["-C", repo, "worktree", "add", "--detach", ready.worktree, "HEAD"], {
      encoding: "utf8",
    });
    spawnSync("git", ["-C", repo, "worktree", "lock", ready.worktree], { encoding: "utf8" });
    rmSync(ready.worktree, { recursive: true, force: true });
    const result = spawnSync(
      process.execPath,
      [AGENT_RUNNER, "--config", configPath, "--prepare-snapshot", "--skip-build"],
      { cwd: REPO_ROOT, encoding: "utf8", timeout: 30_000 },
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(existsSync(codexMarker), false);
    assert.equal(existsSync(join(ready.root, "snapshot-manifest.json")), true);
    const successfulPreparation = JSON.parse(readFileSync(join(directory, "preparation.json"), "utf8"));
    const preparedDatabase = new DatabaseSync(join(ready.root, "goldeneye.db"), { readOnly: true });
    const closedRows = preparedDatabase
      .prepare("SELECT COUNT(*) AS count FROM evidence WHERE value = 'closed'")
      .get().count;
    preparedDatabase.close();
    assert.equal(closedRows, 1, JSON.stringify(successfulPreparation.lifecycle));
    assert.equal(successfulPreparation.snapshot.restore_verified, true);
    assert.equal(
      successfulPreparation.gates.find((gate) => gate.name === "worktree_at_base_after_prepare").passed,
      true,
    );
    assert.deepEqual(successfulPreparation.recovery_evidence, recoveryEvidence);

    writeFileSync(
      fakeGcal,
      `import { mkdirSync, writeFileSync } from "node:fs";\nimport { dirname, join } from "node:path";\nmkdirSync(process.env.GCAL_HOME, { recursive: true });\nwriteFileSync(join(process.env.GCAL_HOME, "projects.json"), "partial-config");\nmkdirSync(dirname(process.env.GOLDENEYE_DB_PATH), { recursive: true });\nwriteFileSync(process.env.GOLDENEYE_DB_PATH, "partial-db");\nprocess.stdout.write('partial stdout');\nprocess.stderr.write('intentional GCAL failure');\nprocess.exit(5);\n`,
    );
    spawnSync("git", ["-C", repo, "add", "dist/main.js"], { encoding: "utf8" });
    spawnSync(
      "git",
      ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fail gcal"],
      { encoding: "utf8" },
    );
    const stable = spawnSync(
      "git",
      ["-C", repo, "worktree", "add", "--detach", ready.worktree, "HEAD"],
      { encoding: "utf8" },
    );
    assert.equal(stable.status, 0, stable.stderr);
    const stableHead = spawnSync("git", ["-C", ready.worktree, "rev-parse", "HEAD"], {
      encoding: "utf8",
    }).stdout.trim();
    const failure = spawnSync(
      process.execPath,
      [AGENT_RUNNER, "--config", configPath, "--prepare-snapshot", "--skip-build"],
      { cwd: REPO_ROOT, encoding: "utf8", timeout: 30_000 },
    );
    assert.notEqual(failure.status, 0);
    assert.match(failure.stderr, /intentional GCAL failure/);
    const preparation = JSON.parse(readFileSync(join(directory, "preparation.json"), "utf8"));
    assert.equal(preparation.eligible_for_scoring, false);
    assert.ok(preparation.provenance);
    assert.ok(preparation.failure_evidence);
    assert.deepEqual(preparation.recovery_evidence, recoveryEvidence);
    assert.equal(preparation.failure_evidence.initializer.exit_code, 5);
    for (const entry of [
      preparation.failure_evidence.initializer.stdout,
      preparation.failure_evidence.initializer.stderr,
      preparation.failure_evidence.initializer.metadata,
      preparation.failure_evidence.resolved_config,
      preparation.failure_evidence.provenance,
      preparation.failure_evidence.live_cache.manifest,
    ]) {
      assert.equal(existsSync(entry.path), true, entry.path);
      assert.match(entry.sha256, /^[a-f0-9]{64}$/);
    }
    assert.equal(existsSync(join(preparation.failure_evidence.live_cache.path, "gcal-state", "projects.json")), true);
    assert.equal(existsSync(join(preparation.failure_evidence.live_cache.path, "goldeneye.db")), true);
    assert.equal(existsSync(join(ready.root, "failure")), false);
    assert.equal(existsSync(ready.worktree), true);
    assert.equal(
      spawnSync("git", ["-C", ready.worktree, "rev-parse", "HEAD"], { encoding: "utf8" }).stdout.trim(),
      stableHead,
    );
    assert.equal(
      spawnSync("git", ["-C", ready.worktree, "status", "--porcelain"], { encoding: "utf8" }).stdout,
      "",
    );
  } finally {
    spawnSync("git", ["-C", repo, "worktree", "remove", "--force", ready.worktree], {
      encoding: "utf8",
    });
    rmSync(directory, { recursive: true, force: true });
  }
});
