import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { parseCodexJsonl } from "./core.mjs";
import {
  agentVerificationPolicy,
  analyzeAgentVerificationCalls,
  applyAgentVerificationPolicy,
  isAgentVerificationCommand,
  resolveOneShotAttemptId,
  resolveOneShotOutput,
  validateOneShotOptions,
} from "./one-shot.mjs";

const BENCH_ROOT = dirname(fileURLToPath(import.meta.url));
const RUNNER = join(BENCH_ROOT, "bin", "benchmark-agent-tasks.mjs");
const LEVEL0_CONFIG = join(BENCH_ROOT, "configs", "spring-sensitive-value-redaction-level0.json");

const SINGLE = {
  enabled: true,
  tasks: [{ id: "redact-level0" }],
  engines: [{ id: "ack", kind: "ack" }],
  cacheModes: ["warm"],
  repetitions: 1,
  activeWorkflowFlags: [],
  attemptId: null,
  skipAgentVerification: false,
};

test("one-shot validates one task, engine, cache mode, and repetition", () => {
  assert.deepEqual(validateOneShotOptions(SINGLE), {
    enabled: true,
    skipAgentVerification: true,
  });

  for (const [field, value, message] of [
    ["tasks", [{ id: "a" }, { id: "b" }], "exactly one task"],
    ["engines", [{ id: "a" }, { id: "b" }], "exactly one engine"],
    ["cacheModes", ["cold", "warm"], "exactly one cache mode"],
    ["repetitions", 2, "exactly one repetition"],
  ]) {
    assert.throws(
      () => validateOneShotOptions({ ...SINGLE, [field]: value }),
      new RegExp(message),
    );
  }
});

test("one-shot rejects canonical workflow flags", () => {
  for (const flag of [
    "--prepare-snapshot",
    "--verify-only",
    "--smoke",
    "--calibration",
    "--audit-report",
  ]) {
    assert.throws(
      () => validateOneShotOptions({ ...SINGLE, activeWorkflowFlags: [flag] }),
      new RegExp(flag),
    );
  }
});

test("attempt-id is valid only in one-shot mode", () => {
  assert.throws(
    () => validateOneShotOptions({ ...SINGLE, enabled: false, attemptId: "manual" }),
    /--attempt-id requires --one-shot/,
  );
  assert.deepEqual(
    validateOneShotOptions({
      ...SINGLE,
      enabled: false,
      skipAgentVerification: true,
    }),
    { enabled: false, skipAgentVerification: true },
  );
  assert.throws(
    () => validateOneShotOptions({ ...SINGLE, attemptId: true }),
    /--attempt-id requires a value/,
  );
});

test("one-shot attempt IDs are sanitized or uniquely generated", () => {
  assert.equal(resolveOneShotAttemptId(" My run #1 "), "my-run-1");
  assert.equal(
    resolveOneShotAttemptId(null, {
      now: () => Date.UTC(2026, 6, 31, 12, 34, 56),
      random: () => "a1b2c3d4",
    }),
    "20260731t123456000z-a1b2c3d4",
  );
});

test("one-shot output defaults to task and attempt isolation", () => {
  const workspace = resolve("D:/bench-workspace");
  assert.equal(
    resolveOneShotOutput({
      workspace,
      taskId: "Level 0 Redaction",
      attemptId: "Try 1",
    }),
    join(workspace, "target", "agent-bench", "level-0-redaction", "one-shot", "try-1", "report.json"),
  );
  assert.equal(
    resolveOneShotOutput({
      workspace,
      taskId: "task",
      attemptId: "attempt",
      explicitOutput: "D:/custom/report.json",
    }),
    resolve("D:/custom/report.json"),
  );
});

test("verification policy has final, explicit prohibitions without limiting ACK", () => {
  const policy = agentVerificationPolicy();
  assert.match(policy, /Do not run/i);
  assert.match(policy, /build/i);
  assert.match(policy, /compile/i);
  assert.match(policy, /test/i);
  assert.match(policy, /lint/i);
  assert.match(policy, /check/i);
  assert.match(policy, /ACK discovery calls are not limited/i);
});

test("verification policy is the final prompt block only when requested", () => {
  assert.equal(applyAgentVerificationPolicy("Task body", false), "Task body");
  const prompt = applyAgentVerificationPolicy("Task body", true);
  assert.match(prompt, /^Task body\n\nOne-shot execution policy/);
  assert.equal(prompt.endsWith(agentVerificationPolicy()), true);
});

test("verification classifier finds clear agent-side verification commands", () => {
  for (const command of [
    "mvn test",
    "./mvnw verify",
    ".\\gradlew.bat build",
    "cargo check",
    "cargo test --workspace",
    "npm run build",
    "npm test",
    "pnpm lint",
    "yarn run test:unit",
    "npx tsc --noEmit",
    "javac Example.java",
    "eslint src",
    "pytest -q",
    "jest --runInBand",
    "vitest run",
    "go test ./...",
    "node --test",
  ]) {
    assert.equal(isAgentVerificationCommand(command), true, command);
  }

  for (const command of [
    "ack status",
    "ack search SensitiveValue",
    "git status --short",
    "git diff",
    "node tools/edit-file.mjs",
    "rg build.gradle",
  ]) {
    assert.equal(isAgentVerificationCommand(command), false, command);
  }
});

test("verification analysis preserves command and exit status", () => {
  assert.deepEqual(
    analyzeAgentVerificationCalls([
      { command: "ack status", exit_code: 0 },
      { command: ["npm", "test"], exitCode: 1 },
    ]),
    {
      agent_verification_calls: [{ command: "npm test", exit_code: 1 }],
      one_shot_compliant: false,
    },
  );
  assert.deepEqual(analyzeAgentVerificationCalls([{ command: "ack inspect Foo", exit_code: 0 }]), {
    agent_verification_calls: [],
    one_shot_compliant: true,
  });
  assert.deepEqual(
    analyzeAgentVerificationCalls(
      Array.from({ length: 100 }, (_, index) => ({ command: `ack get Symbol${index}`, exit_code: 0 })),
    ),
    { agent_verification_calls: [], one_shot_compliant: true },
  );
});

test("Codex telemetry retains command details for compliance analysis", () => {
  const telemetry = parseCodexJsonl(
    JSON.stringify({
      type: "item.completed",
      item: { type: "command_execution", command: "npm test", exit_code: 0 },
    }),
  );
  assert.deepEqual(telemetry.command_events, [{ command: "npm test", exit_code: 0 }]);
});

test("runner help exposes one-shot options", () => {
  const result = spawnSync(process.execPath, [RUNNER, "--help"], { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /--one-shot/);
  assert.match(result.stdout, /--skip-agent-verification/);
  assert.match(result.stdout, /--attempt-id <id>/);
});

test("runner validates one-shot dimensions before repository access", () => {
  const result = spawnSync(
    process.execPath,
    [
      RUNNER,
      "--config",
      LEVEL0_CONFIG,
      "--repo",
      join(BENCH_ROOT, "definitely-missing-repository"),
      "--one-shot",
      "--cache-modes",
      "warm",
      "--repetitions",
      "1",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /exactly one engine/);
  assert.doesNotMatch(result.stderr, /not a git repository/i);
});

test("runner rejects incompatible one-shot workflows before repository access", () => {
  const result = spawnSync(
    process.execPath,
    [
      RUNNER,
      "--config",
      LEVEL0_CONFIG,
      "--repo",
      join(BENCH_ROOT, "definitely-missing-repository"),
      "--one-shot",
      "--engine",
      "vanilla",
      "--cache-modes",
      "warm",
      "--repetitions",
      "1",
      "--smoke",
    ],
    { encoding: "utf8" },
  );
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /incompatible with --smoke/);
  assert.doesNotMatch(result.stderr, /not a git repository/i);
});

test("vanilla one-shot invokes fake Codex and grader once and writes standalone report", () => {
  const directory = mkdtempSync(join(tmpdir(), "agent-bench-one-shot-"));
  const repo = join(directory, "source");
  const fakeBin = join(directory, "fake-bin");
  const fakeCodex = join(fakeBin, "node_modules", "@openai", "codex", "bin", "codex.js");
  const fakeGrader = join(directory, "grader.cjs");
  const configPath = join(directory, "config.json");
  const canonicalOutput = join(directory, "canonical-report.json");
  const oneShotOutput = join(directory, "attempt", "report.json");
  const codexMarker = join(directory, "codex-invocations.jsonl");
  const graderMarker = join(directory, "grader-invocations.txt");
  try {
    mkdirSync(repo, { recursive: true });
    mkdirSync(dirname(fakeCodex), { recursive: true });
    spawnSync("git", ["init", repo], { encoding: "utf8" });
    writeFileSync(join(repo, "TASK.md"), "Return without changing files.\n");
    spawnSync("git", ["-C", repo, "add", "TASK.md"], { encoding: "utf8" });
    const commit = spawnSync(
      "git",
      ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "init"],
      { encoding: "utf8" },
    );
    assert.equal(commit.status, 0, commit.stderr);

    writeFileSync(join(fakeBin, "codex.cmd"), "@echo off\r\n");
    writeFileSync(
      fakeCodex,
      `const fs = require("node:fs");
let prompt = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => { prompt += chunk; });
process.stdin.on("end", () => {
  fs.appendFileSync(process.env.FAKE_CODEX_MARKER, JSON.stringify({ prompt }) + "\\n");
  const args = process.argv.slice(2);
  const outputIndex = args.indexOf("-o");
  if (outputIndex >= 0) fs.writeFileSync(args[outputIndex + 1], JSON.stringify({ summary: "done" }));
  if (process.env.EMIT_VERIFICATION === "1") {
    process.stdout.write(JSON.stringify({ type: "item.completed", item: { type: "command_execution", command: "npm test", exit_code: 0 } }) + "\\n");
  }
  process.stdout.write(JSON.stringify({ type: "turn.completed", usage: { input_tokens: 10, output_tokens: 2 } }) + "\\n");
  process.exitCode = Number(process.env.FAKE_CODEX_EXIT || 0);
});
`,
    );
    writeFileSync(
      fakeGrader,
      `require("node:fs").appendFileSync(process.env.GRADER_MARKER, "1\\n");\nprocess.exit(Number(process.env.FAKE_GRADER_EXIT || 0));\n`,
    );
    writeFileSync(canonicalOutput, '{"sentinel":true}\n');
    writeFileSync(
      configPath,
      JSON.stringify({
        repo,
        output: canonicalOutput,
        codex_command: "codex",
        repetitions: 3,
        cache_modes: ["warm"],
        timeout_ms: 10_000,
        tasks: [{
          id: "task",
          prompt_file: join(repo, "TASK.md"),
          grader: {
            command: process.execPath,
            args: [fakeGrader],
            env: { GRADER_MARKER: graderMarker },
          },
        }],
        engines: [{ id: "vanilla", kind: "vanilla", cache_modes: ["warm"] }],
      }),
    );

    const runnerArgs = [
        RUNNER,
        "--config",
        configPath,
        "--one-shot",
        "--engine",
        "vanilla",
        "--cache-modes",
        "warm",
        "--repetitions",
        "1",
        "--attempt-id",
        "integration",
        "--out",
        oneShotOutput,
      ];
    const runnerOptions = {
        cwd: resolve(BENCH_ROOT, "../.."),
        encoding: "utf8",
        env: {
          ...process.env,
          FAKE_CODEX_MARKER: codexMarker,
          PATH: `${fakeBin};${process.env.PATH}`,
        },
        timeout: 30_000,
      };
    const dryRun = spawnSync(process.execPath, [...runnerArgs, "--dry-run"], runnerOptions);
    assert.equal(dryRun.status, 0, dryRun.stderr);
    assert.equal(JSON.parse(dryRun.stdout).mode, "one-shot");
    assert.equal(existsSync(codexMarker), false);
    assert.equal(existsSync(graderMarker), false);
    assert.equal(existsSync(oneShotOutput), false);

    const result = spawnSync(process.execPath, runnerArgs, runnerOptions);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(readFileSync(codexMarker, "utf8").trim().split(/\r?\n/).length, 1);
    assert.equal(readFileSync(graderMarker, "utf8").trim().split(/\r?\n/).length, 1);
    assert.deepEqual(JSON.parse(readFileSync(canonicalOutput, "utf8")), { sentinel: true });
    assert.equal(existsSync(oneShotOutput), true);
    const report = JSON.parse(readFileSync(oneShotOutput, "utf8"));
    assert.equal(report.mode, "one-shot");
    assert.equal(report.attempt_id, "integration");
    assert.equal(report.qualified, false);
    assert.equal(report.qualification, "skipped");
    assert.equal(report.agent_verification_policy, "skip");
    assert.equal(report.snapshot_refreshed, false);
    assert.equal(report.model_invocations, 1);
    assert.equal(report.grader_invocations, 1);
    assert.equal(report.one_shot_compliant, true);
    assert.equal(report.runs.length, 1);
    assert.equal(report.runs[0].model_invocations, 1);
    assert.equal(report.runs[0].grader_invocations, 1);
    const invocation = JSON.parse(readFileSync(codexMarker, "utf8").trim());
    assert.equal(invocation.prompt.trimEnd().endsWith(agentVerificationPolicy()), true);

    const repeated = spawnSync(process.execPath, runnerArgs, runnerOptions);
    assert.notEqual(repeated.status, 0);
    assert.match(repeated.stderr, /output already exists/);
    assert.equal(readFileSync(codexMarker, "utf8").trim().split(/\r?\n/).length, 1);
    assert.equal(readFileSync(graderMarker, "utf8").trim().split(/\r?\n/).length, 1);

    const noncompliantOutput = join(directory, "noncompliant", "report.json");
    const noncompliantArgs = [...runnerArgs];
    noncompliantArgs[noncompliantArgs.indexOf("--attempt-id") + 1] = "noncompliant";
    noncompliantArgs[noncompliantArgs.indexOf("--out") + 1] = noncompliantOutput;
    const noncompliant = spawnSync(process.execPath, noncompliantArgs, {
      ...runnerOptions,
      env: { ...runnerOptions.env, EMIT_VERIFICATION: "1" },
    });
    assert.equal(noncompliant.status, 0, noncompliant.stderr);
    const noncompliantReport = JSON.parse(readFileSync(noncompliantOutput, "utf8"));
    assert.equal(noncompliantReport.success, true);
    assert.equal(noncompliantReport.one_shot_compliant, false);
    assert.deepEqual(noncompliantReport.agent_verification_calls, [
      { command: "npm test", exit_code: 0 },
    ]);
    assert.equal(readFileSync(codexMarker, "utf8").trim().split(/\r?\n/).length, 2);
    assert.equal(readFileSync(graderMarker, "utf8").trim().split(/\r?\n/).length, 2);

    const graderFailureOutput = join(directory, "grader-failure", "report.json");
    const graderFailureArgs = [...runnerArgs];
    graderFailureArgs[graderFailureArgs.indexOf("--attempt-id") + 1] = "grader-failure";
    graderFailureArgs[graderFailureArgs.indexOf("--out") + 1] = graderFailureOutput;
    const graderFailure = spawnSync(process.execPath, graderFailureArgs, {
      ...runnerOptions,
      env: { ...runnerOptions.env, FAKE_GRADER_EXIT: "7" },
    });
    assert.notEqual(graderFailure.status, 0);
    const graderFailureReport = JSON.parse(readFileSync(graderFailureOutput, "utf8"));
    assert.equal(graderFailureReport.success, false);
    assert.equal(graderFailureReport.model_invocations, 1);
    assert.equal(graderFailureReport.grader_invocations, 1);
    assert.equal(graderFailureReport.runs[0].grader_exit_code, 7);
    assert.equal(readFileSync(codexMarker, "utf8").trim().split(/\r?\n/).length, 3);
    assert.equal(readFileSync(graderMarker, "utf8").trim().split(/\r?\n/).length, 3);

    const codexFailureOutput = join(directory, "codex-failure", "report.json");
    const codexFailureArgs = [...runnerArgs];
    codexFailureArgs[codexFailureArgs.indexOf("--attempt-id") + 1] = "codex-failure";
    codexFailureArgs[codexFailureArgs.indexOf("--out") + 1] = codexFailureOutput;
    const codexFailure = spawnSync(process.execPath, codexFailureArgs, {
      ...runnerOptions,
      env: { ...runnerOptions.env, FAKE_CODEX_EXIT: "9" },
    });
    assert.notEqual(codexFailure.status, 0);
    const codexFailureReport = JSON.parse(readFileSync(codexFailureOutput, "utf8"));
    assert.equal(codexFailureReport.success, false);
    assert.equal(codexFailureReport.model_invocations, 1);
    assert.equal(codexFailureReport.grader_invocations, 1);
    assert.equal(codexFailureReport.runs[0].codex_exit_code, 9);
    assert.equal(readFileSync(codexMarker, "utf8").trim().split(/\r?\n/).length, 4);
    assert.equal(readFileSync(graderMarker, "utf8").trim().split(/\r?\n/).length, 4);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("ACK one-shot auto-refresh failure stops before Codex", () => {
  const directory = mkdtempSync(join(tmpdir(), "agent-bench-one-shot-ack-"));
  const repo = join(directory, "source");
  const fakeAck = join(repo, "dist", "main.js");
  const configPath = join(directory, "config.json");
  const codexMarker = join(directory, "codex-spawned");
  const output = join(directory, "attempt", "report.json");
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
    mkdirSync(dirname(fakeAck), { recursive: true });
    writeFileSync(join(repo, "TASK.md"), "No-op task.\n");
    writeFileSync(join(repo, "package-lock.json"), '{"lockfileVersion":3}\n');
    writeFileSync(fakeAck, 'process.stderr.write("intentional ACK refresh failure"); process.exit(5);\n');
    spawnSync("git", ["-C", repo, "add", "."], { encoding: "utf8" });
    const commit = spawnSync(
      "git",
      ["-C", repo, "-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "init"],
      { encoding: "utf8" },
    );
    assert.equal(commit.status, 0, commit.stderr);
    writeFileSync(
      configPath,
      JSON.stringify({
        repo,
        output,
        codex_command: codexMarker,
        repetitions: 1,
        cache_modes: ["warm"],
        timeout_ms: 10_000,
        tasks: [{
          id: "task",
          prompt_file: join(repo, "TASK.md"),
          grader: { command: process.execPath },
        }],
        engines: [{
          id: "ack",
          kind: "ack",
          command: process.execPath,
          args: [fakeAck],
          backend_command: process.execPath,
          cache_modes: ["warm"],
        }],
        ready_snapshot: ready,
      }),
    );
    mkdirSync(ready.allowed_worktree_root, { recursive: true });
    const stable = spawnSync(
      "git",
      ["-C", repo, "worktree", "add", "--detach", ready.worktree, "HEAD"],
      { encoding: "utf8" },
    );
    assert.equal(stable.status, 0, stable.stderr);
    spawnSync("git", ["-C", repo, "worktree", "lock", ready.worktree], { encoding: "utf8" });
    rmSync(ready.worktree, { recursive: true, force: true });

    const result = spawnSync(
      process.execPath,
      [
        RUNNER,
        "--config",
        configPath,
        "--one-shot",
        "--engine",
        "ack",
        "--cache-modes",
        "warm",
        "--repetitions",
        "1",
        "--attempt-id",
        "refresh-failure",
        "--out",
        output,
      ],
      { cwd: resolve(BENCH_ROOT, "../.."), encoding: "utf8", timeout: 30_000 },
    );
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /intentional ACK refresh failure/);
    assert.equal(existsSync(codexMarker), false);
    assert.equal(existsSync(output), false);
    const preparation = JSON.parse(readFileSync(join(dirname(output), "preparation.json"), "utf8"));
    assert.equal(preparation.eligible_for_scoring, false);
    assert.match(preparation.error, /intentional ACK refresh failure/);
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
