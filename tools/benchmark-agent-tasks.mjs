#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  createWriteStream,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { performance } from "node:perf_hooks";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import {
  buildRunMatrix,
  codexSandboxArgs,
  emptyTelemetry,
  expandTokens,
  loadConfig,
  protocolViolationsForEngine,
  resolveRepositoryGate,
  resolveRunLayout,
  sanitizeId,
  shouldPrimeIndex,
  summarizeRuns,
  tomlInlineTable,
  tomlString,
  accumulateCodexLine,
} from "./agent-bench/core.mjs";
import {
  assertNoWriterArtifacts,
  buildManifest,
  copyRegularTree,
  createReadySnapshot,
  restoreReadySnapshot,
  verifyTreeAgainstManifest,
  verifyReadySnapshot,
  waitForNoWriterArtifacts,
} from "./agent-bench/snapshot.mjs";
import {
  closeWritableStream,
  scoreRunDurations,
  spawnWithTimer,
  stopTimerAtClose,
} from "./agent-bench/timing.mjs";
import {
  captureRepositoryProvenance,
  compareProvenance,
  selectDependencyLock,
  sha256,
} from "./agent-bench/provenance.mjs";

const workspace = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const flags = parseFlags(process.argv.slice(2));

if (flags.has("--help")) {
  console.log(`Usage:
  node tools/benchmark-agent-tasks.mjs --config <json> [options]

Options:
  --repo <path>             override repository
  --base-ref <git-ref>      override pinned base ref
  --model <model>           Codex model for both lanes
  --reasoning <level>       Codex reasoning effort for both lanes
  --repetitions <n>         default from config, or 1
  --cache-modes <list>      cold,warm
  --task <id>               select one task
  --engine <id>             select one engine
  --seed <n>                deterministic random run order
  --timeout-ms <n>          per-agent timeout
  --out <path>              report JSON
  --skip-build              use the existing Goldeneye release binary
  --dry-run                 validate and print the matrix only
  --prepare-snapshot        create the immutable ACK ready snapshot and exit
  --verify-only             validate frozen candidate, source, and snapshot without Codex
  --smoke                   run one unscored ACK candidate and held-out grader
  --keep-worktrees          keep agent worktrees
  --keep-caches             keep MCP caches
`);
  process.exit(0);
}

const configFlag = flags.get("--config");
if (!configFlag) fail("--config is required");

const { config, path: configPath } = loadConfig(configFlag);
config.repo = resolve(flags.get("--repo") ?? config.repo);
config.base_ref = flags.get("--base-ref") ?? config.base_ref ?? "HEAD";
config.model = flags.get("--model") ?? config.model;
config.reasoning = flags.get("--reasoning") ?? config.reasoning;
config.repetitions = positiveInteger(flags.get("--repetitions") ?? config.repetitions ?? 1, "repetitions");
config.seed = integer(flags.get("--seed") ?? config.seed ?? 20260718, "seed");
config.timeout_ms = positiveInteger(flags.get("--timeout-ms") ?? config.timeout_ms ?? 1_800_000, "timeout-ms");
config.output = resolve(flags.get("--out") ?? config.output ?? join(workspace, "target", "agent-bench", "report.json"));
config.cache_modes = String(flags.get("--cache-modes") ?? config.cache_modes ?? "cold,warm")
  .split(",")
  .map((value) => value.trim())
  .filter(Boolean);
if (config.cache_modes.some((mode) => !["cold", "warm"].includes(mode))) {
  fail("--cache-modes accepts only cold,warm");
}
config.tasks = select(config.tasks, flags.get("--task"), "task");
config.engines = select(config.engines, flags.get("--engine"), "engine");

const baseCommit = git(config.repo, ["rev-parse", `${config.base_ref}^{commit}`]).trim();
const repoName = sanitizeId(config.repo.split(/[\\/]/).filter(Boolean).at(-1));
const runId = `${timestamp()}-${baseCommit.slice(0, 10)}`;
const shortRunId = `${baseCommit.slice(0, 8)}-${Date.now().toString(36)}`;
const runRoot = resolve(
  config.run_root ??
    join(workspace, "target", "agent-bench", "runs", runId),
);
const worktreeRoot = resolve(
  config.worktree_root ?? join(dirname(config.repo), ".gab", shortRunId),
);
const cacheRoot = resolve(config.cache_root ?? join(dirname(config.repo), ".gab-cache", shortRunId));
const matrix = buildRunMatrix({
  tasks: config.tasks,
  engines: config.engines,
  cacheModes: config.cache_modes,
  repetitions: config.repetitions,
  seed: config.seed,
});

if (flags.has("--dry-run")) {
  console.log(
    JSON.stringify(
      {
        config: configPath,
        repository: config.repo,
        base_commit: baseCommit,
        run_root: runRoot,
        worktree_root: worktreeRoot,
        cache_root: cacheRoot,
        runs: matrix.map(({ id, task, engine, cacheMode, repetition }) => ({
          id,
          task: task.id,
          engine: engine.id,
          cache_mode: cacheMode,
          repetition,
        })),
      },
      null,
      2,
    ),
  );
  process.exit(0);
}

if (!flags.has("--skip-build") && config.engines.some((engine) => engine.id === "goldeneye")) {
  console.log("Building Goldeneye release binary...");
  const fullGrammarPack = join(workspace, "target", "goldeneye-grammars");
  if (!process.env.GOLDENEYE_GRAMMAR_PACK_DIR && existsSync(fullGrammarPack)) {
    process.env.GOLDENEYE_GRAMMAR_PACK_DIR = fullGrammarPack;
  }
  runChecked("cargo", ["build", "--release", "-p", "goldeneye"], workspace);
}

if (flags.has("--prepare-snapshot") || flags.has("--verify-only") || flags.has("--smoke")) {
  const artifacts = resolvePreparationArtifacts(config);
  let preparation = null;
  try {
    preparation = flags.has("--prepare-snapshot")
      ? await prepareReadySnapshot({ baseCommit, config, repoName })
      : readPreparation(artifacts.preparation);
    if (flags.has("--verify-only")) {
      preparation.verification = await verifyPreparedSnapshot({
        baseCommit,
        config,
        expectedCandidate: preparation.provenance?.candidate ?? null,
      });
    }
    if (flags.has("--smoke")) {
      preparation.smoke = await runSmoke({
        artifacts,
        baseCommit,
        config,
        expectedCandidate: preparation.provenance?.candidate ?? null,
        repoName,
      });
    }
    preparation.eligible_for_scoring = Boolean(
      preparation.snapshot?.restore_verified &&
      preparation.smoke?.success &&
      preparation.smoke?.snapshot_unchanged &&
      preparation.smoke?.candidate_unchanged &&
      preparation.smoke?.source_repository_clean,
    );
    preparation.completed_at = new Date().toISOString();
  } catch (error) {
    preparation ??= error.preparation ?? { schema_version: 1, gates: [], provenance: null, snapshot: null };
    preparation.eligible_for_scoring = false;
    preparation.error = errorMessage(error);
    preparation.completed_at = new Date().toISOString();
    persistReport(artifacts.preparation, preparation);
    throw error;
  }
  if (preparation.provenance) persistReport(artifacts.provenance, preparation.provenance);
  persistReport(artifacts.preparation, preparation);
  console.log(`Preparation gates: ${preparation.eligible_for_scoring ? "ELIGIBLE" : "NOT ELIGIBLE"} ${artifacts.preparation}`);
  process.exit(preparation.eligible_for_scoring || !flags.has("--smoke") ? 0 : 1);
}

mkdirSync(runRoot, { recursive: true });
const report = {
  generated_at: new Date().toISOString(),
  config: configPath,
  repository: config.repo,
  repository_name: repoName,
  base_commit: baseCommit,
  worktree_root: worktreeRoot,
  cache_root: cacheRoot,
  settings: {
    model: config.model ?? null,
    reasoning: config.reasoning ?? null,
    repetitions: config.repetitions,
    cache_modes: config.cache_modes,
    seed: config.seed,
    timeout_ms: config.timeout_ms,
    codex_full_access: config.codex_full_access === true,
    randomized_order: matrix.map((run) => run.id),
    engines: config.engines.map((engine) => ({
      id: engine.id,
      kind: engine.kind,
      command: engine.command ?? null,
      cache_modes: engine.cache_modes ?? config.cache_modes,
    })),
  },
  runs: [],
  summary: [],
};

for (const [position, run] of matrix.entries()) {
  console.log(`[${position + 1}/${matrix.length}] ${run.id}`);
  const result = await executeRun(run, {
    baseCommit,
    cacheRoot,
    config,
    repoName,
    runRoot,
    worktreeRoot,
  });
  report.runs.push(result);
  report.summary = summarizeRuns(report.runs);
  persistReport(config.output, report);
  console.log(
    `  ${result.success ? "PASS" : "FAIL"} wall=${formatMs(result.wall_ms)} tokens=${result.total_tokens} grader=${result.grader_exit_code}`,
  );
}

report.completed_at = new Date().toISOString();
report.summary = summarizeRuns(report.runs);
persistReport(config.output, report);
console.log(`Agent benchmark artifact: ${config.output}`);

async function prepareReadySnapshot({ baseCommit, config, repoName }) {
  const readySnapshot = config.ready_snapshot;
  if (!readySnapshot) throw new Error("--prepare-snapshot requires ready_snapshot configuration");
  const ackEngine = config.engines.find((engine) => engine.kind === "ack");
  if (!ackEngine) throw new Error("--prepare-snapshot requires an ACK engine");

  const startedAt = performance.now();
  const provenance = captureBenchmarkProvenance({ config, configPath });
  const gates = [];
  recordGate(gates, "candidate_preparation_start", {
    expected: provenance.candidate,
    observed: provenance.candidate,
    passed: true,
  });
  assertRepositoryAtBase(config.repo, baseCommit);
  recordGate(gates, "source_repository_at_base_before_prepare", {
    expected: baseCommit,
    observed: baseCommit,
    passed: true,
  });
  removeWorktreeIfRegistered(
    config.repo,
    readySnapshot.worktree,
    readySnapshot.allowed_worktree_root,
  );
  mkdirSync(dirname(readySnapshot.worktree), { recursive: true });
  runChecked("git", ["-C", config.repo, "worktree", "add", "--detach", readySnapshot.worktree, baseCommit]);
  try {
    linkWorkspaceDependencies(config.repo, readySnapshot.worktree);
    rmIfInside(readySnapshot.live_cache, readySnapshot.allowed_cache_root);
    mkdirSync(readySnapshot.live_cache, { recursive: true });
    const engine = engineRuntime(
      ackEngine,
      readySnapshot.worktree,
      readySnapshot.live_cache,
      repoName,
      dirname(config.repo),
    );
    await initializeAckForSnapshot(
      engine,
      readySnapshot.worktree,
      config.preindex_timeout_ms ?? 600_000,
    );
    await waitForNoWriterArtifacts(readySnapshot.live_cache);
    const manifest = await createReadySnapshot({
      liveCache: readySnapshot.live_cache,
      snapshotRoot: readySnapshot.root,
      allowedCacheRoot: readySnapshot.allowed_cache_root,
      allowedSnapshotRoot: readySnapshot.allowed_snapshot_root,
      projectRoot: readySnapshot.worktree,
      baseRef: baseCommit,
    });
    await verifyReadySnapshot({
      snapshotRoot: readySnapshot.root,
      allowedSnapshotRoot: readySnapshot.allowed_snapshot_root,
      expected: manifest,
      expectedProjectRoot: readySnapshot.worktree,
      expectedBaseRef: baseCommit,
    });
    await restoreReadySnapshot({
      snapshotRoot: readySnapshot.root,
      liveCache: readySnapshot.live_cache,
      allowedCacheRoot: readySnapshot.allowed_cache_root,
      allowedSnapshotRoot: readySnapshot.allowed_snapshot_root,
      expectedProjectRoot: readySnapshot.worktree,
      expectedBaseRef: baseCommit,
    });
    await verifyReadySnapshot({
      snapshotRoot: readySnapshot.root,
      allowedSnapshotRoot: readySnapshot.allowed_snapshot_root,
      expected: manifest,
      expectedProjectRoot: readySnapshot.worktree,
      expectedBaseRef: baseCommit,
    });
    recordGate(gates, "snapshot_create_restore_verify", {
      expected: manifest,
      observed: manifest,
      passed: true,
    });
    assertRepositoryAtBase(config.repo, baseCommit);
    const after = captureBenchmarkProvenance({ config, configPath });
    assertCandidateUnchanged(provenance.candidate, after.candidate, "post-preparation");
    recordGate(gates, "candidate_post_preparation", {
      expected: provenance.candidate,
      observed: after.candidate,
      passed: true,
    });
    recordGate(gates, "source_repository_at_base_after_prepare", {
      expected: baseCommit,
      observed: baseCommit,
      passed: true,
    });
    return {
      schema_version: 1,
      prepared_at: new Date().toISOString(),
      preparation_ms: performance.now() - startedAt,
      provenance,
      gates,
      snapshot: {
        manifest_sha256: createHash("sha256").update(JSON.stringify(manifest)).digest("hex"),
        file_count: manifest.file_count,
        byte_count: manifest.byte_count,
        project_root: manifest.project_root,
        base_ref: manifest.base_ref,
        restore_verified: true,
      },
    };
  } catch (error) {
    recordGate(gates, "ack_initialize", {
      expected: { exit_code: 0 },
      observed: error.initializer ?? errorMessage(error),
      passed: false,
    });
    const failureEvidence = await preservePreparationFailure({
      baseCommit,
      config,
      error,
      liveCache: readySnapshot.live_cache,
      provenance,
      worktree: readySnapshot.worktree,
    });
    error.preparation = {
      schema_version: 1,
      prepared_at: new Date().toISOString(),
      preparation_ms: performance.now() - startedAt,
      provenance,
      gates,
      snapshot: null,
      failure_evidence: failureEvidence,
    };
    throw error;
  } finally {
    removeWorktreeIfRegistered(
      config.repo,
      readySnapshot.worktree,
      readySnapshot.allowed_worktree_root,
    );
    rmIfInside(readySnapshot.live_cache, readySnapshot.allowed_cache_root);
  }
}

async function preservePreparationFailure({ baseCommit, config, error, liveCache, provenance, worktree }) {
  const root = join(resolvePreparationArtifacts(config).root, "failure", timestamp());
  mkdirSync(root, { recursive: true });
  const resolvedConfig = writeFailureEvidenceFile(root, "resolved-config.json", `${JSON.stringify(config, null, 2)}\n`);
  const provenanceArtifact = writeFailureEvidenceFile(root, "provenance.json", `${JSON.stringify(provenance, null, 2)}\n`);
  const errorArtifact = writeFailureEvidenceFile(root, "error.txt", `${errorMessage(error)}\n`);
  const initializer = preserveInitializerEvidence(root, error.initializer);
  let liveCacheEvidence = null;
  if (existsSync(liveCache)) {
    const copiedCache = join(root, "live-cache");
    mkdirSync(copiedCache, { recursive: true });
    await copyRegularTree(liveCache, copiedCache);
    const manifest = await buildManifest(copiedCache, { projectRoot: worktree, baseRef: baseCommit });
    await verifyTreeAgainstManifest(copiedCache, manifest);
    const manifestArtifact = writeFailureEvidenceFile(
      root,
      "live-cache-manifest.json",
      `${JSON.stringify(manifest, null, 2)}\n`,
    );
    liveCacheEvidence = {
      path: copiedCache,
      copy_mode: "copyFile",
      file_count: manifest.file_count,
      byte_count: manifest.byte_count,
      manifest: manifestArtifact,
    };
  }
  return {
    directory: root,
    resolved_config: resolvedConfig,
    provenance: provenanceArtifact,
    error: errorArtifact,
    initializer,
    live_cache: liveCacheEvidence,
  };
}

function preserveInitializerEvidence(root, initializer) {
  if (!initializer) return null;
  const stdout = writeFailureEvidenceFile(root, "ack-init.stdout.log", initializer.stdout ?? "");
  const stderr = writeFailureEvidenceFile(root, "ack-init.stderr.log", initializer.stderr ?? "");
  const metadata = writeFailureEvidenceFile(
    root,
    "ack-init.json",
    `${JSON.stringify({
      exit_code: initializer.exit_code,
      duration_ms: initializer.duration_ms,
      error: initializer.error,
    }, null, 2)}\n`,
  );
  return {
    exit_code: initializer.exit_code,
    duration_ms: initializer.duration_ms,
    stdout,
    stderr,
    metadata,
  };
}

function writeFailureEvidenceFile(root, name, contents) {
  const path = join(root, name);
  const bytes = Buffer.isBuffer(contents) ? contents : Buffer.from(String(contents));
  writeFileSync(path, bytes);
  return { path, bytes: bytes.length, sha256: sha256(bytes) };
}

function resolvePreparationArtifacts(config) {
  const root = resolve(
    config.artifact_root ?? (config.name
      ? join(workspace, "target", "agent-bench", config.name)
      : dirname(config.output)),
  );
  return {
    root,
    preparation: resolve(config.preparation_output ?? join(root, "preparation.json")),
    provenance: resolve(config.provenance_output ?? join(root, "provenance.json")),
  };
}

function readPreparation(path) {
  if (!existsSync(path)) throw new Error(`Preparation artifact not found: ${path}`);
  return JSON.parse(readFileSync(path, "utf8"));
}

function captureBenchmarkProvenance({ config, configPath: currentConfigPath }) {
  const goldeneyeFull = captureRepositoryProvenance({
    repo: workspace,
    selectedFiles: [
      "crates/application/goldeneye-query/src/engine/search.rs",
      "target/release/goldeneye.exe",
    ],
  });
  const ackEngine = config.engines.find((engine) => engine.kind === "ack");
  if (!ackEngine) throw new Error("Candidate provenance requires an ACK engine");
  const mainModule = (ackEngine.args ?? []).find((arg) => /(?:^|[\\/])dist[\\/]main\.js$/i.test(arg));
  if (!mainModule) throw new Error(`ACK engine ${ackEngine.id} must name dist/main.js in args`);
  const ackRoot = resolve(dirname(mainModule), "..");
  const ack = captureRepositoryProvenance({
    repo: ackRoot,
    selectedFiles: ["dist/main.js", selectDependencyLock(ackRoot)],
  });
  const operationalFiles = [
    currentConfigPath,
    ...config.tasks.flatMap((task) => [
      task.prompt_file,
      ...(task.grader?.args ?? []).filter((arg) => /\.(?:mjs|cjs|js|ps1)$/i.test(arg)),
    ]),
  ].map((file) => resolve(file));
  const harnessFiles = [
    "tools/benchmark-agent-tasks.mjs",
    "tools/agent-bench/core.mjs",
    "tools/agent-bench/snapshot.mjs",
    "tools/agent-bench/timing.mjs",
    "tools/agent-bench/provenance.mjs",
  ];
  return {
    captured_at: new Date().toISOString(),
    candidate: {
      goldeneye: goldeneyeFull,
      ack,
    },
    harness: {
      repository: captureRepositoryProvenance({ repo: workspace, selectedFiles: harnessFiles }),
      goldeneye_worktree: goldeneyeFull,
      operational_files: operationalFiles
        .sort()
        .map((file) => ({ path: file, bytes: statSync(file).size, sha256: sha256(readFileSync(file)) })),
    },
  };
}

function assertCandidateUnchanged(expected, observed, phase) {
  const comparison = compareProvenance(expected, observed);
  if (!comparison.equal) {
    throw new Error(
      `candidate provenance mismatch ${phase}: ${comparison.field}; expected=${JSON.stringify(comparison.expected)} observed=${JSON.stringify(comparison.observed)}`,
    );
  }
  return comparison;
}

function recordGate(gates, name, { expected, observed, passed, duration_ms = 0 }) {
  gates.push({
    name,
    passed,
    expected,
    observed,
    duration_ms,
    observed_at: new Date().toISOString(),
  });
}

async function verifyPreparedSnapshot({ baseCommit, config, expectedCandidate }) {
  const startedAt = performance.now();
  if (!config.ready_snapshot) throw new Error("--verify-only requires ready_snapshot configuration");
  assertRepositoryAtBase(config.repo, baseCommit);
  const observed = captureBenchmarkProvenance({ config, configPath });
  if (expectedCandidate) assertCandidateUnchanged(expectedCandidate, observed.candidate, "verify-only");
  const manifest = await verifyReadySnapshot({
    snapshotRoot: config.ready_snapshot.root,
    allowedSnapshotRoot: config.ready_snapshot.allowed_snapshot_root,
    expectedProjectRoot: config.ready_snapshot.worktree,
    expectedBaseRef: baseCommit,
  });
  return {
    verified_at: new Date().toISOString(),
    duration_ms: performance.now() - startedAt,
    source_repository_clean: true,
    candidate_unchanged: true,
    snapshot: {
      manifest_sha256: sha256(JSON.stringify(manifest)),
      file_count: manifest.file_count,
      byte_count: manifest.byte_count,
      project_root: manifest.project_root,
      base_ref: manifest.base_ref,
    },
  };
}

async function runSmoke({ artifacts, baseCommit, config, expectedCandidate, repoName }) {
  const readySnapshot = config.ready_snapshot;
  if (!readySnapshot) throw new Error("--smoke requires ready_snapshot configuration");
  const task = config.tasks.at(0);
  const engine = config.engines.find((entry) => entry.kind === "ack");
  if (!task || !engine) throw new Error("--smoke requires one task and an ACK engine");

  const preSmoke = captureBenchmarkProvenance({ config, configPath });
  if (expectedCandidate) assertCandidateUnchanged(expectedCandidate, preSmoke.candidate, "pre-smoke");
  const verified = await verifyPreparedSnapshot({ baseCommit, config, expectedCandidate: preSmoke.candidate });
  const smokeRoot = resolve(artifacts.root, "smoke", timestamp());
  const result = await executeRun(
    {
      id: `smoke-${task.id}-${engine.id}`,
      task,
      engine,
      cacheMode: "warm",
      repetition: 0,
    },
    {
      baseCommit,
      cacheRoot: readySnapshot.allowed_cache_root,
      config,
      repoName,
      runRoot: smokeRoot,
      worktreeRoot: readySnapshot.allowed_worktree_root,
    },
  );
  const manifest = await verifyReadySnapshot({
    snapshotRoot: readySnapshot.root,
    allowedSnapshotRoot: readySnapshot.allowed_snapshot_root,
    expectedProjectRoot: readySnapshot.worktree,
    expectedBaseRef: baseCommit,
  });
  assertRepositoryAtBase(config.repo, baseCommit);
  const postSmoke = captureBenchmarkProvenance({ config, configPath });
  assertCandidateUnchanged(preSmoke.candidate, postSmoke.candidate, "post-smoke");
  return {
    smoke_artifact_dir: smokeRoot,
    unscored: true,
    result,
    success: result.success,
    source_repository_clean: true,
    candidate_unchanged: true,
    snapshot_unchanged: true,
    snapshot_manifest_sha256: sha256(JSON.stringify(manifest)),
    pre_smoke_verification: verified,
    pre_smoke_candidate: preSmoke.candidate,
    post_smoke_candidate: postSmoke.candidate,
  };
}

async function executeRun(run, context) {
  const runDir = join(context.runRoot, run.id);
  const laneKey = createHash("sha256").update(run.id).digest("hex").slice(0, 12);
  const layout = resolveRunLayout({
    kind: run.engine.kind,
    readySnapshot: context.config.ready_snapshot,
    runId: laneKey,
    worktreeRoot: context.worktreeRoot,
    cacheRoot: context.cacheRoot,
  });
  const { cacheDir, usesReadySnapshot, worktree } = layout;
  const worktreeRoot = usesReadySnapshot
    ? context.config.ready_snapshot.allowed_worktree_root
    : context.worktreeRoot;
  const cacheRoot = usesReadySnapshot
    ? context.config.ready_snapshot.allowed_cache_root
    : context.cacheRoot;
  mkdirSync(runDir, { recursive: true });
  const maintenanceStarted = performance.now();
  if (!flags.has("--keep-caches")) rmIfInside(cacheDir, cacheRoot);
  mkdirSync(cacheDir, { recursive: true });

  let worktreeAdded = false;
  let maintenanceMs = null;
  let setupMs = null;
  let preindexMs = null;
  let snapshotManifest = null;
  let agentResult = null;
  let graderResult = null;
  let diff = "";
  let status = "";
  let patchStats = { files: 0, insertions: 0, deletions: 0 };
  let cacheBytes = 0;
  let engine = null;
  let engineMetrics = {};
  try {
    removeWorktreeIfRegistered(context.config.repo, worktree, worktreeRoot);
    mkdirSync(dirname(worktree), { recursive: true });
    runChecked("git", ["-C", context.config.repo, "worktree", "add", "--detach", worktree, context.baseCommit]);
    worktreeAdded = true;
    linkWorkspaceDependencies(context.config.repo, worktree);

    if (usesReadySnapshot) {
      snapshotManifest = await restoreReadySnapshot({
        snapshotRoot: context.config.ready_snapshot.root,
        liveCache: cacheDir,
        allowedCacheRoot: context.config.ready_snapshot.allowed_cache_root,
        allowedSnapshotRoot: context.config.ready_snapshot.allowed_snapshot_root,
        expectedProjectRoot: worktree,
        expectedBaseRef: context.baseCommit,
      });
      assertRepositoryAtBase(
        resolveRepositoryGate({
          sourceRepository: context.config.repo,
          worktree,
          usesReadySnapshot,
        }),
        context.baseCommit,
      );
    }

    const setupStarted = performance.now();
    engine = engineRuntime(
      run.engine,
      worktree,
      cacheDir,
      context.repoName,
      dirname(context.config.repo),
    );
    prepareEngine(engine, context.config.preindex_timeout_ms ?? 600_000);
    setupMs = performance.now() - setupStarted;
    if (run.cacheMode === "warm" && shouldPrimeIndex({ kind: engine.kind, usesReadySnapshot })) {
      const started = performance.now();
      await primeIndex(engine, worktree, context.config.preindex_timeout_ms ?? 600_000);
      preindexMs = performance.now() - started;
    }

    const prompt = composePrompt(run.task, run.cacheMode, engine);
    writeFileSync(join(runDir, "prompt.txt"), prompt);
    maintenanceMs = performance.now() - maintenanceStarted;
    agentResult = await runCodex({
      cacheMode: run.cacheMode,
      config: context.config,
      engine,
      prompt,
      runDir,
      worktree,
    });

    git(worktree, ["add", "-N", "--", "."]);
    diff = git(worktree, ["diff", "--binary", "--no-ext-diff"]);
    status = git(worktree, ["status", "--short"]);
    patchStats = readPatchStats(worktree);
    writeFileSync(join(runDir, "patch.diff"), diff);
    writeFileSync(join(runDir, "status.txt"), status);

    graderResult = runGrader(run.task, {
      repo: context.config.repo,
      runDir,
      taskDir: dirname(run.task.prompt_file),
      worktree,
    });
  } catch (error) {
    maintenanceMs ??= performance.now() - maintenanceStarted;
    agentResult ??= failedAgentResult(error);
    graderResult ??= { exit_code: null, duration_ms: null, error: errorMessage(error) };
  } finally {
    cacheBytes = directorySize(cacheDir);
    if (engine?.kind === "ack") {
      engineMetrics = {
        ack_registry_exists: existsSync(join(engine.ackHome, "projects.json")),
        cbm_decoy_files: directoryFileCount(engine.cbmDecoy),
        goldeneye_db_bytes: existsSync(engine.goldeneyeDb)
          ? statSync(engine.goldeneyeDb).size
          : 0,
      };
    }
    if (worktreeAdded && !flags.has("--keep-worktrees")) {
      removeWorktreeIfRegistered(context.config.repo, worktree, worktreeRoot);
    }
    if (!flags.has("--keep-caches")) {
      rmIfInside(cacheDir, cacheRoot);
    }
  }

  const telemetry = agentResult.telemetry ?? emptyTelemetry();
  const protocolViolations = protocolViolationsForEngine(
    telemetry.protocol_violations,
    run.engine.kind,
  );
  const graderPassed = graderResult.exit_code === 0;
  const graderMs = graderResult.duration_ms;
  const durations = Number.isFinite(agentResult.duration_ms) && Number.isFinite(graderMs)
    ? scoreRunDurations({ maintenanceMs, wallMs: agentResult.duration_ms, graderMs })
    : {
        maintenance_ms: maintenanceMs,
        wall_ms: agentResult.duration_ms,
        grader_ms: graderMs,
        completion_ms: agentResult.duration_ms,
        verified_e2e_ms: null,
      };
  const success =
    agentResult.exit_code === 0 &&
    !agentResult.timed_out &&
    graderPassed &&
    protocolViolations.length === 0;
  return {
    id: run.id,
    task_id: run.task.id,
    engine: run.engine.id,
    cache_mode: run.cacheMode,
    repetition: run.repetition,
    success,
    codex_exit_code: agentResult.exit_code,
    timed_out: agentResult.timed_out,
    wall_ms: durations.wall_ms,
    maintenance_ms: durations.maintenance_ms,
    setup_ms: setupMs,
    preindex_ms: preindexMs,
    completion_ms: durations.completion_ms,
    verified_e2e_ms: durations.verified_e2e_ms,
    grader_exit_code: graderResult.exit_code,
    grader_ms: graderResult.duration_ms,
    error: agentResult.error ?? graderResult.error ?? null,
    input_tokens: telemetry.input_tokens,
    cached_input_tokens: telemetry.cached_input_tokens,
    output_tokens: telemetry.output_tokens,
    reasoning_output_tokens: telemetry.reasoning_output_tokens,
    total_tokens: telemetry.input_tokens + telemetry.output_tokens,
    tool_calls: telemetry.tool_calls,
    mcp_calls: telemetry.mcp_calls,
    mcp_calls_by_server: telemetry.mcp_calls_by_server,
    mcp_failures: telemetry.mcp_failures,
    index_failures: telemetry.index_failures,
    ack_calls: telemetry.ack_calls,
    ack_failures: telemetry.ack_failures,
    command_calls: telemetry.command_calls,
    event_bytes: telemetry.jsonl_bytes,
    cache_bytes: cacheBytes,
    durations,
    snapshot: snapshotManifest && {
      manifest_sha256: createHash("sha256").update(JSON.stringify(snapshotManifest)).digest("hex"),
      file_count: snapshotManifest.file_count,
      byte_count: snapshotManifest.byte_count,
      project_root: snapshotManifest.project_root,
      base_ref: snapshotManifest.base_ref,
      restore_verified: true,
    },
    ...engineMetrics,
    protocol_violations: protocolViolations,
    patch_bytes: Buffer.byteLength(diff),
    patch_files: patchStats.files,
    patch_insertions: patchStats.insertions,
    patch_deletions: patchStats.deletions,
    dirty_paths: status.split(/\r?\n/).filter(Boolean).length,
    artifact_dir: runDir,
  };
}

function composePrompt(task, cacheMode, engine) {
  const taskPrompt = readFileSync(task.prompt_file, "utf8").trim();
  const sourceLanguage = task.source_language ?? "TypeScript, TSX, or Rust";
  const sourceExtensions = (task.source_extensions ?? [".ts", ".tsx", ".rs"]).join(", ");
  const discoveryInstructions =
    engine.kind === "vanilla"
      ? "- No code-memory MCP is available. Use ordinary local shell and file tools for discovery."
      : engine.kind === "ack"
        ? `- Use only the ack CLI to discover and read ${sourceLanguage} source. Direct Goldeneye MCP tools are not available in this lane.\n- Do not inspect ${sourceExtensions} source through shell/text-search commands or direct file-read tools. Commands such as rg, grep, Select-String, Get-Content, cat, head, tail, sed, and git show are protocol violations. Use ack search/symbol/get/inspect/callers/callees/arch/status instead. git status, git diff, edits, and build/test commands remain allowed.`
        : `- Use only the assigned ${engine.id} MCP tools to discover and read ${sourceLanguage} source.\n- Do not inspect ${sourceExtensions} source through shell/text-search commands or direct file-read tools. Commands such as rg, grep, Select-String, Get-Content, cat, head, tail, sed, and git show are protocol violations. Use the assigned MCP's semantic search and source tools instead. git status, git diff, edits, and build/test commands remain allowed.`;
  const cacheInstructions =
    cacheMode === "none"
      ? "- Cache condition: none; this lane has no code-memory engine."
      : engine.kind === "ack"
        ? cacheMode === "cold"
          ? "- Cache condition: cold. Run ack init once from the repository root before other ACK commands. Do not set ACK_PROJECT."
          : "- Cache condition: warm. ack init completed before this turn; use cwd-based ACK project resolution. Do not set ACK_PROJECT."
        : `- Cache condition: ${cacheMode}. Do not make assumptions about whether the repository is indexed; check through MCP when useful.`;
  const ackInstruction =
    engine.kind === "ack"
      ? "- Use one ACK command path per discovery need; stop discovery once enough evidence exists."
      : "- Do not invoke the ack CLI; it is not part of this benchmark lane.";
  return `${task.common_prompt ?? ""}

You are participating in a controlled code-maintenance benchmark.
- Work directly in the current repository and complete the task.
- You may inspect, edit, and test the code.
${discoveryInstructions}
${ackInstruction}
- Do not read or use Superfastpower skills. Do not spend time loading skill files; work directly.
- Do not commit, push, or modify files outside the current repository.
- Preserve unrelated user changes.
${cacheInstructions}

Task:
${taskPrompt}
`.trimStart();
}

function engineRuntime(engine, worktree, cacheDir, repoName, allowedRoot) {
  if (engine.kind === "vanilla") {
    return {
      args: [],
      command: null,
      environment: {},
      id: engine.id,
      kind: engine.kind,
      repoName,
    };
  }
  const command = resolveExecutable(engine.command);
  if (engine.kind === "ack") {
    const ackHome = join(cacheDir, "ack-state");
    const cbmDecoy = join(cacheDir, "cbm-decoy");
    const goldeneyeDb = join(cacheDir, "goldeneye.db");
    return {
      command,
      args: engine.args ?? [],
      ackHome,
      cbmDecoy,
      environment: {
        ACK_HOME: ackHome,
        ACK_MCP_COMMAND: resolveExecutable(engine.backend_command),
        CBM_ALLOWED_ROOT: allowedRoot,
        CBM_CACHE_DIR: cbmDecoy,
        GOLDENEYE_DB_PATH: goldeneyeDb,
        GOLDENEYE_PROJECT_ROOT: worktree,
        ...(engine.env ?? {}),
      },
      goldeneyeDb,
      id: engine.id,
      kind: engine.kind,
      repoName,
      unsetEnvironment: ["ACK_MCP_URL", "ACK_PROJECT"],
    };
  }
  if (engine.kind === "serena") {
    const serenaHome = join(cacheDir, "serena-home");
    return {
      command,
      args: [
        "start-mcp-server",
        "--context=codex",
        "--project",
        worktree,
        "--enable-web-dashboard",
        "false",
        "--open-web-dashboard",
        "false",
        "--enable-gui-log-window",
        "false",
        "--log-level",
        "ERROR",
        ...(engine.args ?? []),
      ],
      environment: { SERENA_HOME: serenaHome, ...(engine.env ?? {}) },
      id: engine.id,
      kind: engine.kind,
      language: engine.language ?? "java",
      mcpServerName: engine.mcp_server_name ?? "serena",
      repoName,
      serenaHome,
    };
  }
  const environment = {
    CBM_ALLOWED_ROOT: allowedRoot,
    CBM_CACHE_DIR: cacheDir,
    CBM_SEMANTIC_ENABLED: "1",
    CBM_SEMANTIC_THRESHOLD: "0.82",
    GOLDENEYE_PROJECT_ROOT: worktree,
    GOLDENEYE_DB_PATH: join(cacheDir, "goldeneye.db"),
    GOLDENEYE_MCP_RESPONSE_MODE: "text",
    ...(engine.env ?? {}),
  };
  return {
    command,
    args: engine.args ?? [],
    environment,
    id: engine.id,
    kind: engine.kind,
    mcpServerName: engine.mcp_server_name ?? "codebase_memory_mcp",
    repoName,
  };
}

function prepareEngine(engine, timeoutMs) {
  if (engine.kind !== "serena") return;
  mkdirSync(engine.serenaHome, { recursive: true });
  const init = spawnSync(engine.command, ["init", "-b", "LSP"], {
    encoding: "utf8",
    env: processEnvironment(engine),
    shell: false,
    timeout: timeoutMs,
    windowsHide: true,
  });
  if (init.status !== 0) {
    throw new Error(`Serena init failed: ${tail(init.stderr || init.stdout)}`);
  }
  const configPath = join(engine.serenaHome, "serena_config.yml");
  const projectData = join(engine.serenaHome, "projects", "$projectFolderName", ".serena")
    .replace(/\\/g, "/");
  const config = readFileSync(configPath, "utf8");
  const isolated = config.replace(
    /^project_serena_folder_location:.*$/m,
    `project_serena_folder_location: "${projectData}"`,
  );
  if (isolated === config) {
    throw new Error("Serena config is missing project_serena_folder_location");
  }
  writeFileSync(configPath, isolated);
}

async function runCodex({ cacheMode, config, engine, prompt, runDir, worktree }) {
  const jsonlPath = join(runDir, "codex.jsonl");
  const stderrPath = join(runDir, "codex.stderr.log");
  const finalMessagePath = join(runDir, "final-message.txt");
  const args = [
    "exec",
    "--json",
    "--ephemeral",
    "--ignore-user-config",
    "--ignore-rules",
    "--color",
    "never",
  ];
  args.push(...codexSandboxArgs({
    fullAccess: config.codex_full_access === true,
    worktree,
  }));
  args.push("-C", worktree);
  if (engine.kind !== "vanilla" && engine.kind !== "ack") {
    const serverName = engine.mcpServerName;
    args.push(
      "-c",
      `mcp_servers.${serverName}.command=${tomlString(engine.command)}`,
      "-c",
      `mcp_servers.${serverName}.args=${JSON.stringify(engine.args)}`,
      "-c",
      `mcp_servers.${serverName}.env=${tomlInlineTable(engine.environment)}`,
      "-c",
      `mcp_servers.${serverName}.default_tools_approval_mode="approve"`,
    );
  }
  args.push(
    "-o",
    finalMessagePath,
    "--output-schema",
    join(workspace, "tools", "agent-bench", "final-response.schema.json"),
  );
  if (config.model) args.push("-m", config.model);
  if (config.reasoning) args.push("-c", `model_reasoning_effort=${tomlString(config.reasoning)}`);
  args.push("-");

  const telemetry = emptyTelemetry();
  const jsonlStream = createWriteStream(jsonlPath, { encoding: "utf8" });
  const stderrStream = createWriteStream(stderrPath, { encoding: "utf8" });
  const codex = resolveCodexLaunch(config.codex_command ?? "codex");
  const measured = spawnWithTimer(() =>
    spawn(codex.command, [...codex.prefixArgs, ...args], {
      cwd: worktree,
      env: processEnvironment(engine, { AGENT_BENCH_CACHE_MODE: cacheMode }),
      shell: false,
      windowsHide: true,
      stdio: ["pipe", "pipe", "pipe"],
    }),
  );
  const child = measured.child;
  child.stderr.pipe(stderrStream);
  const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
  lines.on("line", (line) => {
    jsonlStream.write(`${line}\n`);
    accumulateCodexLine(telemetry, line);
  });
  child.stdin.end(prompt);

  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    killProcessTree(child.pid);
  }, config.timeout_ms);
  const outcome = await new Promise((resolveOutcome) => {
    child.on("error", (error) => resolveOutcome({
      durationMs: stopTimerAtClose(measured),
      exitCode: null,
      error,
    }));
    child.on("close", (exitCode) => resolveOutcome({
      durationMs: stopTimerAtClose(measured),
      exitCode,
      error: null,
    }));
  });
  clearTimeout(timer);
  lines.close();
  await Promise.all([closeWritableStream(jsonlStream), closeWritableStream(stderrStream)]);
  return {
    duration_ms: outcome.durationMs,
    error: outcome.error ? errorMessage(outcome.error) : null,
    exit_code: outcome.exitCode,
    telemetry,
    timed_out: timedOut,
  };
}

async function initializeAckForSnapshot(engine, worktree, timeoutMs) {
  const startedAt = performance.now();
  const child = spawn(engine.command, [...engine.args, "init", worktree], {
    cwd: worktree,
    env: processEnvironment(engine),
    shell: false,
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  let stdout = "";
  let stderr = "";
  child.stdout.on("data", (chunk) => { stdout += chunk; });
  child.stderr.on("data", (chunk) => { stderr += chunk; });
  const timer = setTimeout(() => killProcessTree(child.pid), timeoutMs);
  const outcome = await new Promise((resolveOutcome) => {
    child.on("error", (error) => resolveOutcome({ error, exitCode: null }));
    child.on("close", (exitCode) => resolveOutcome({ error: null, exitCode }));
  });
  clearTimeout(timer);
  const initializer = {
    exit_code: outcome.exitCode,
    duration_ms: performance.now() - startedAt,
    error: outcome.error ? errorMessage(outcome.error) : null,
    stdout,
    stderr,
  };
  if (outcome.error || outcome.exitCode !== 0) {
    const error = new Error(
      `ACK init failed: ${outcome.error ? errorMessage(outcome.error) : tail(`${stdout}${stderr}`)}`,
    );
    error.initializer = initializer;
    throw error;
  }
  const descendants = processChildren(child.pid);
  if (descendants.length > 0) {
    throw new Error(`ACK initializer left child processes running: ${descendants.join(", ")}`);
  }
}

async function primeIndex(engine, worktree, timeoutMs) {
  if (engine.kind === "ack") {
    const result = spawnSync(engine.command, [...engine.args, "init", worktree], {
      cwd: worktree,
      encoding: "utf8",
      env: processEnvironment(engine),
      shell: false,
      timeout: timeoutMs,
      windowsHide: true,
    });
    if (result.status !== 0) {
      throw new Error(`ACK init failed: ${tail(result.stderr || result.stdout)}`);
    }
    return;
  }
  if (engine.kind === "serena") {
    const result = spawnSync(
      engine.command,
      [
        "project",
        "index",
        worktree,
        "--language",
        engine.language,
        "--log-level",
        "ERROR",
      ],
      {
        encoding: "utf8",
        env: processEnvironment(engine),
        shell: false,
        timeout: timeoutMs,
        windowsHide: true,
      },
    );
    if (result.status !== 0) {
      throw new Error(`Warm preindex failed for ${engine.id}: ${tail(result.stderr || result.stdout)}`);
    }
    return;
  }
  const child = spawn(engine.command, engine.args, {
    cwd: worktree,
    env: processEnvironment(engine),
    shell: false,
    windowsHide: true,
    stdio: ["pipe", "pipe", "pipe"],
  });
  let nextId = 1;
  const pending = new Map();
  let stderr = "";
  child.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
    if (stderr.length > 20_000) stderr = stderr.slice(-20_000);
  });
  const lines = createInterface({ input: child.stdout, crlfDelay: Infinity });
  lines.on("line", (line) => {
    try {
      const message = JSON.parse(line);
      if (message.id !== undefined && pending.has(message.id)) {
        const { resolve: resolvePending, reject } = pending.get(message.id);
        pending.delete(message.id);
        if (message.error) reject(new Error(JSON.stringify(message.error)));
        else resolvePending(message.result);
      }
    } catch {
      // MCP servers may emit non-protocol diagnostics; stderr is preferred but tolerated.
    }
  });
  const request = (method, params) => {
    const id = nextId++;
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
    return new Promise((resolveRequest, reject) => pending.set(id, { reject, resolve: resolveRequest }));
  };
  const timeout = setTimeout(() => {
    for (const { reject } of pending.values()) reject(new Error("MCP preindex timed out"));
    pending.clear();
    killProcessTree(child.pid);
  }, timeoutMs);
  try {
    await request("initialize", {
      protocolVersion: "2024-11-05",
      capabilities: {},
      clientInfo: { name: "goldeneye-agent-bench", version: "1.0.0" },
    });
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`);
    const indexResult = await request("tools/call", {
      name: "index_repository",
      arguments: { repo_path: worktree, mode: "full", persistence: false },
    });
    if (indexResult?.isError) throw new Error(JSON.stringify(indexResult.content ?? indexResult));
  } catch (error) {
    throw new Error(`Warm preindex failed for ${engine.id}: ${errorMessage(error)}\n${stderr.slice(-2000)}`);
  } finally {
    clearTimeout(timeout);
    child.stdin.end();
    const exited = await waitForExit(child, 3000);
    if (!exited) killProcessTree(child.pid);
    lines.close();
  }
}

function runGrader(task, tokens) {
  const grader = task.grader;
  const command = expandTokens(grader.command, tokens);
  const args = expandTokens(grader.args ?? [], tokens);
  const stdoutPath = join(tokens.runDir, "grader.stdout.log");
  const stderrPath = join(tokens.runDir, "grader.stderr.log");
  const started = performance.now();
  const result = spawnSync(resolveExecutable(command), args, {
    cwd: tokens.worktree,
    env: { ...process.env, ...(expandTokens(grader.env ?? {}, tokens)) },
    encoding: "utf8",
    maxBuffer: grader.max_buffer_bytes ?? 10 * 1024 * 1024,
    shell: false,
    timeout: grader.timeout_ms ?? 900_000,
    windowsHide: true,
  });
  writeFileSync(stdoutPath, result.stdout ?? "");
  writeFileSync(stderrPath, result.stderr ?? "");
  return {
    duration_ms: performance.now() - started,
    error: result.error ? errorMessage(result.error) : null,
    exit_code: result.status,
  };
}

function readPatchStats(worktree) {
  const output = git(worktree, ["diff", "--numstat"]);
  let files = 0;
  let insertions = 0;
  let deletions = 0;
  for (const line of output.split(/\r?\n/)) {
    if (!line) continue;
    const [added, removed] = line.split("\t");
    files += 1;
    if (/^\d+$/.test(added)) insertions += Number(added);
    if (/^\d+$/.test(removed)) deletions += Number(removed);
  }
  return { files, insertions, deletions };
}

function removeWorktreeIfRegistered(repo, worktree, allowedRoot) {
  const resolvedRepo = resolve(repo);
  const resolvedWorktree = resolve(worktree);
  const resolvedAllowedRoot = resolve(allowedRoot);
  if (!isInside(resolvedWorktree, resolvedAllowedRoot)) {
    throw new Error(`Refusing worktree cleanup outside ${resolvedAllowedRoot}: ${resolvedWorktree}`);
  }
  spawnSync("git", ["-C", resolvedRepo, "worktree", "remove", "--force", resolvedWorktree], {
    encoding: "utf8",
    windowsHide: true,
  });
  spawnSync("git", ["-C", resolvedRepo, "worktree", "prune"], { encoding: "utf8", windowsHide: true });
}

function linkWorkspaceDependencies(repo, worktree) {
  const source = join(repo, "node_modules");
  const target = join(worktree, "node_modules");
  if (!existsSync(source) || existsSync(target)) return;
  symlinkSync(source, target, process.platform === "win32" ? "junction" : "dir");
}

function rmIfInside(target, parent) {
  const absoluteTarget = resolve(target);
  const absoluteParent = resolve(parent);
  if (!isInside(absoluteTarget, absoluteParent)) {
    throw new Error(`Refusing recursive delete outside ${absoluteParent}: ${absoluteTarget}`);
  }
  rmSync(absoluteTarget, { force: true, recursive: true });
}

function isInside(child, parent) {
  const prefix = `${parent}${process.platform === "win32" ? "\\" : "/"}`;
  return child.startsWith(prefix) && child !== parent;
}

function resolveExecutable(command) {
  if (!command) throw new Error("Missing executable command");
  if (existsSync(command) && statSync(command).isFile()) return resolve(command);
  if (process.platform === "win32" && existsSync(`${command}.exe`)) return resolve(`${command}.exe`);
  if (process.platform === "win32" && !command.includes("\\") && !command.includes("/")) {
    const found = spawnSync("where.exe", [command], { encoding: "utf8", windowsHide: true });
    if (found.status === 0) {
      const candidates = found.stdout.split(/\r?\n/).filter(Boolean);
      const native = candidates.find((candidate) => candidate.toLowerCase().endsWith(".exe"));
      if (native) return native;
    }
  }
  return command;
}

function resolveCodexLaunch(command) {
  if (process.platform !== "win32" || /[\\/]/.test(command)) {
    return { command: resolveExecutable(command), prefixArgs: [] };
  }
  const found = spawnSync("where.exe", ["codex.cmd"], { encoding: "utf8", windowsHide: true });
  if (found.status === 0) {
    const shim = found.stdout.split(/\r?\n/).find(Boolean);
    if (shim) {
      const script = join(dirname(shim), "node_modules", "@openai", "codex", "bin", "codex.js");
      if (existsSync(script)) return { command: process.execPath, prefixArgs: [script] };
    }
  }
  return { command: resolveExecutable(command), prefixArgs: [] };
}

function git(cwd, args) {
  const result = spawnSync("git", ["-C", cwd, ...args], {
    encoding: "utf8",
    maxBuffer: 50 * 1024 * 1024,
    windowsHide: true,
  });
  if (result.status !== 0) throw new Error(`git ${args.join(" ")} failed: ${result.stderr}`);
  return result.stdout;
}

function assertRepositoryAtBase(repo, baseCommit) {
  const status = git(repo, ["status", "--porcelain"]);
  if (status.trim()) throw new Error(`source repository is not clean: ${resolve(repo)}`);
  const head = git(repo, ["rev-parse", "HEAD"]).trim();
  if (head !== baseCommit) {
    throw new Error(`source repository commit mismatch: expected ${baseCommit}, got ${head}`);
  }
}

function runChecked(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, encoding: "utf8", windowsHide: true });
  if (result.status !== 0) throw new Error(`${command} ${args.join(" ")} failed: ${result.stderr}`);
  return result.stdout;
}

function killProcessTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(pid), "/T", "/F"], { windowsHide: true });
  } else {
    try {
      process.kill(-pid, "SIGKILL");
    } catch {
      try {
        process.kill(pid, "SIGKILL");
      } catch {
        // Process already exited.
      }
    }
  }
}

function waitForExit(child, timeoutMs) {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true);
  return new Promise((resolveWait) => {
    const timer = setTimeout(() => resolveWait(false), timeoutMs);
    child.once("close", () => {
      clearTimeout(timer);
      resolveWait(true);
    });
  });
}

function processChildren(parentPid) {
  if (!parentPid) return [];
  const result = process.platform === "win32"
    ? spawnSync(
        "powershell",
        [
          "-NoProfile",
          "-Command",
          `Get-CimInstance Win32_Process -Filter 'ParentProcessId = ${Number(parentPid)}' | Select-Object -ExpandProperty ProcessId`,
        ],
        { encoding: "utf8", windowsHide: true },
      )
    : spawnSync("ps", ["-o", "pid=", "--ppid", String(parentPid)], {
        encoding: "utf8",
        windowsHide: true,
      });
  if (result.status !== 0) return [];
  return String(result.stdout)
    .split(/\r?\n/)
    .map((value) => value.trim())
    .filter((value) => /^\d+$/.test(value));
}

function directorySize(path) {
  if (!existsSync(path)) return 0;
  let total = 0;
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    if (entry.isDirectory()) total += directorySize(child);
    else if (entry.isFile()) total += statSync(child).size;
  }
  return total;
}

function directoryFileCount(path) {
  if (!existsSync(path)) return 0;
  let count = 0;
  for (const entry of readdirSync(path, { withFileTypes: true })) {
    const child = join(path, entry.name);
    count += entry.isDirectory() ? directoryFileCount(child) : 1;
  }
  return count;
}

function processEnvironment(engine, extra = {}) {
  const environment = { ...process.env, ...engine.environment, ...extra };
  for (const key of engine.unsetEnvironment ?? []) delete environment[key];
  return environment;
}

function failedAgentResult(error) {
  return {
    duration_ms: null,
    error: errorMessage(error),
    exit_code: null,
    telemetry: emptyTelemetry(),
    timed_out: false,
  };
}

function persistReport(path, report) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(report, null, 2)}\n`);
}

function parseFlags(args) {
  const parsed = new Map();
  for (let index = 0; index < args.length; index += 1) {
    const name = args[index];
    const value = args[index + 1];
    if (value !== undefined && !value.startsWith("--")) {
      parsed.set(name, value);
      index += 1;
    } else {
      parsed.set(name, true);
    }
  }
  return parsed;
}

function select(items, id, kind) {
  if (!id) return items;
  const selected = items.filter((item) => item.id === id);
  if (selected.length === 0) fail(`Unknown ${kind}: ${id}`);
  return selected;
}

function positiveInteger(value, name) {
  const parsed = integer(value, name);
  if (parsed <= 0) fail(`${name} must be positive`);
  return parsed;
}

function integer(value, name) {
  const parsed = Number(value);
  if (!Number.isInteger(parsed)) fail(`${name} must be an integer`);
  return parsed;
}

function timestamp() {
  return new Date().toISOString().replace(/[:.]/g, "-");
}

function formatMs(value) {
  return Number.isFinite(value) ? `${Math.round(value)}ms` : "n/a";
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

function tail(value, limit = 2_000) {
  const text = String(value ?? "").trim();
  return text.length > limit ? text.slice(-limit) : text;
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
