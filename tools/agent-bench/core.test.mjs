import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  buildRunMatrix,
  isDirectSourceReadCommand,
  loadConfig,
  parseCodexJsonl,
  protocolViolationsForEngine,
  resolveRepositoryGate,
  resolveRunLayout,
  sanitizeId,
  shouldPrimeIndex,
  summarizeRuns,
  tomlInlineTable,
} from "./core.mjs";

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

test("parseCodexJsonl extracts cumulative usage, tool calls, bytes, and violations", () => {
  const lines = [
    JSON.stringify({ type: "thread.started", thread_id: "abc" }),
    JSON.stringify({
      type: "item.completed",
      item: { type: "mcp_tool_call", server: "codebase_memory_mcp", name: "search_graph" },
    }),
    JSON.stringify({
      type: "item.completed",
      item: { type: "command_execution", command: "ack search Foo" },
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
  assert.equal(telemetry.ack_calls, 1);
  assert.equal(telemetry.ack_failures, 0);
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

test("ACK lanes allow ACK commands while preserving direct source-read violations", () => {
  const telemetry = parseCodexJsonl(
    [
      {
        type: "item.completed",
        item: {
          type: "command_execution",
          command: `pwsh.exe -Command 'ack search SecurityConfig'`,
          exit_code: 0,
        },
      },
      {
        type: "item.completed",
        item: { type: "command_execution", command: "ack get Missing", exit_code: 1 },
      },
      {
        type: "item.completed",
        item: { type: "command_execution", command: "Get-Content src/main/java/App.java" },
      },
    ]
      .map(JSON.stringify)
      .join("\n"),
  );
  assert.equal(telemetry.ack_calls, 2);
  assert.equal(telemetry.ack_failures, 1);
  assert.deepEqual(
    protocolViolationsForEngine(telemetry.protocol_violations, "ack").map((item) => item.type),
    ["direct_source_read"],
  );
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

test("tomlInlineTable quotes Windows paths and environment keys", () => {
  assert.equal(
    tomlInlineTable({ CBM_CACHE_DIR: "D:\\cache path" }),
    '{ "CBM_CACHE_DIR" = "D:\\\\cache path" }',
  );
});

test("loadConfig normalizes and validates ready snapshot paths", () => {
  const directory = resolve("tools", "agent-bench");
  const configPath = join(directory, `ready-snapshot-${process.pid}.json`);
  const config = {
    repo: "../spring-framework",
    output: "out/report.json",
    tasks: [{ id: "task", prompt_file: "task.md", grader: { command: "grader.mjs" } }],
    engines: [{ id: "ack", kind: "ack", command: "ack", backend_command: "goldeneye" }],
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

test("ready snapshots use stable ACK paths and skip priming", () => {
  const readySnapshot = {
    root: "D:\\Dev\\IdeaProjects\\goldeneye-tool\\target\\agent-bench\\snapshots\\spring-stringutils",
    worktree: "D:\\Dev\\IdeaProjects\\.gab\\spring-stringutils-worktree",
    live_cache: "D:\\Dev\\IdeaProjects\\.gab-cache\\spring-stringutils-live",
  };
  assert.deepEqual(
    resolveRunLayout({ kind: "ack", readySnapshot, runId: "candidate-1" }),
    {
      worktree: readySnapshot.worktree,
      cacheDir: readySnapshot.live_cache,
      usesReadySnapshot: true,
    },
  );
  assert.equal(shouldPrimeIndex({ kind: "ack", usesReadySnapshot: true }), false);

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
  const fakeAck = join(repo, "dist", "main.js");
  const configPath = join(directory, "config.json");
  const codexMarker = join(directory, "codex-spawned");
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
      fakeAck,
      `import { mkdirSync, writeFileSync } from "node:fs";\nimport { dirname, join } from "node:path";\nmkdirSync(process.env.ACK_HOME, { recursive: true });\nwriteFileSync(join(process.env.ACK_HOME, "projects.json"), "{}");\nmkdirSync(dirname(process.env.GOLDENEYE_DB_PATH), { recursive: true });\nwriteFileSync(process.env.GOLDENEYE_DB_PATH, "ready");\n`,
    );
    spawnSync("git", ["-C", repo, "add", "README.md"], { encoding: "utf8" });
    spawnSync("git", ["-C", repo, "add", "package-lock.json"], { encoding: "utf8" });
    spawnSync("git", ["-C", repo, "add", "dist/main.js"], { encoding: "utf8" });
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
          id: "ack",
          kind: "ack",
          command: process.execPath,
          args: [fakeAck],
          backend_command: process.execPath,
        }],
        ready_snapshot: ready,
      }),
    );
    const result = spawnSync(
      process.execPath,
      [resolve("tools/benchmark-agent-tasks.mjs"), "--config", configPath, "--prepare-snapshot", "--skip-build"],
      { cwd: resolve(), encoding: "utf8", timeout: 30_000 },
    );
    assert.equal(result.status, 0, result.stderr);
    assert.equal(existsSync(codexMarker), false);
    assert.equal(existsSync(join(ready.root, "snapshot-manifest.json")), true);
    assert.equal(JSON.parse(readFileSync(join(directory, "preparation.json"), "utf8")).snapshot.restore_verified, true);

    writeFileSync(
      fakeAck,
      `import { mkdirSync, writeFileSync } from "node:fs";\nimport { dirname, join } from "node:path";\nmkdirSync(process.env.ACK_HOME, { recursive: true });\nwriteFileSync(join(process.env.ACK_HOME, "projects.json"), "partial-config");\nmkdirSync(dirname(process.env.GOLDENEYE_DB_PATH), { recursive: true });\nwriteFileSync(process.env.GOLDENEYE_DB_PATH, "partial-db");\nprocess.stdout.write('partial stdout');\nprocess.stderr.write('intentional ACK failure');\nprocess.exit(5);\n`,
    );
    spawnSync("git", ["-C", repo, "add", "dist/main.js"], { encoding: "utf8" });
    spawnSync(
      "git",
      ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fail ack"],
      { encoding: "utf8" },
    );
    const failure = spawnSync(
      process.execPath,
      [resolve("tools/benchmark-agent-tasks.mjs"), "--config", configPath, "--prepare-snapshot", "--skip-build"],
      { cwd: resolve(), encoding: "utf8", timeout: 30_000 },
    );
    assert.notEqual(failure.status, 0);
    assert.match(failure.stderr, /intentional ACK failure/);
    const preparation = JSON.parse(readFileSync(join(directory, "preparation.json"), "utf8"));
    assert.equal(preparation.eligible_for_scoring, false);
    assert.ok(preparation.provenance);
    assert.ok(preparation.failure_evidence);
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
    assert.equal(existsSync(join(preparation.failure_evidence.live_cache.path, "ack-state", "projects.json")), true);
    assert.equal(existsSync(join(preparation.failure_evidence.live_cache.path, "goldeneye.db")), true);
    assert.equal(existsSync(join(ready.root, "failure")), false);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
