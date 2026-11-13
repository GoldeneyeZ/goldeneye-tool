#!/usr/bin/env node
// Replays the recorded patch of agent runs against `gcal check`, without a model.
//
// Every one-shot run leaves the agent's final diff (`patch.diff`) and the
// grader's verdict (`report.json`). For each run of the given tasks this script
// applies that diff to a clone of the task's repository at its base commit,
// indexed at that commit, runs `gcal check` on the result and prints what the
// check reports beyond parsing, next to the verdict. A report on a run that
// passed is a false alarm; no report on a run that failed is a miss.
//
// Usage:
//   node tools/agent-bench/bin/replay-check.mjs --state <dir> [--attempts <regex>] [--types]
//        [--gcal-entry <main.js>] [--goldeneye <exe>] <task>...
//
// --state     directory for the clones and their indexes (created; reused)
// --attempts  attempt ids to replay (default: every attempt of the task)
// --types     link the repository's node_modules into the clone, as the
//             harness does, and run `gcal check --types`
// --reindex   index the clones again (after a change of the backend binary)
//
// The task's repository is only read: it is cloned with `git clone`.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, rmdirSync, symlinkSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const benchRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = resolve(benchRoot, "..", "..");

const options = { state: undefined, attempts: ".", types: false, reindex: false, entry: join(repoRoot, "agent", "dist", "main.js"), goldeneye: join(repoRoot, "target", "release", process.platform === "win32" ? "goldeneye.exe" : "goldeneye") };
const tasks = [];
const args = process.argv.slice(2);
for (let index = 0; index < args.length; index += 1) {
  const arg = args[index];
  if (arg === "--state") options.state = args[++index];
  else if (arg === "--attempts") options.attempts = args[++index];
  else if (arg === "--gcal-entry") options.entry = args[++index];
  else if (arg === "--goldeneye") options.goldeneye = args[++index];
  else if (arg === "--types") options.types = true;
  else if (arg === "--reindex") options.reindex = true;
  else tasks.push(arg);
}
if (options.state === undefined || tasks.length === 0) {
  console.error("usage: replay-check.mjs --state <dir> [--attempts <regex>] [--types] [--gcal-entry <main.js>] [--goldeneye <exe>] <task>...");
  process.exit(2);
}
const state = resolve(options.state);
const attempts = new RegExp(options.attempts);
mkdirSync(state, { recursive: true });

const REPORTED =
  /^# (removed or renamed|.* was deleted and is still imported|calls that match no declared constructor|type check \(tsc --noEmit\): \d+ new error|HEAD could not be type-checked)/;

function git(cwd, gitArgs) {
  return spawnSync("git", ["-C", cwd, ...gitArgs], { encoding: "utf8", maxBuffer: 256 * 1024 * 1024 });
}

function find(directory, name, found = []) {
  for (const item of readdirSync(directory, { withFileTypes: true })) {
    const path = join(directory, item.name);
    if (item.isDirectory()) find(path, name, found);
    else if (item.name === name) found.push(path);
  }
  return found;
}

const totals = { replayed: 0, failed: 0, failedReported: 0, passedReported: 0 };
for (const task of tasks) {
  const configPath = join(benchRoot, "configs", `${task}.json`);
  const config = JSON.parse(readFileSync(configPath, "utf8"));
  const repository = resolve(dirname(configPath), config.repo);
  const clone = join(state, task);
  const environment = {
    ...process.env,
    ...(config.engines.find((engine) => engine.kind === "gcal")?.env ?? {}),
    GCAL_HOME: join(state, `${task}-home`),
    GOLDENEYE_DB_PATH: join(state, `${task}.db`),
    GCAL_BACKEND: "goldeneye",
    GCAL_GOLDENEYE_COMMAND: options.goldeneye,
    GCAL_DAEMON: "off",
  };
  const gcal = (gcalArgs) =>
    spawnSync(process.execPath, [options.entry, ...gcalArgs], { cwd: clone, env: environment, encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });

  const fresh = !existsSync(clone);
  if (fresh) {
    const cloned = spawnSync("git", ["clone", "-q", "-c", "core.longpaths=true", "--no-hardlinks", repository, clone], { encoding: "utf8" });
    if (cloned.status !== 0 || git(clone, ["checkout", "-q", config.base_ref]).status !== 0) {
      console.log(`${task}: could not clone ${repository} at ${config.base_ref}: ${cloned.stderr.slice(0, 200)}`);
      continue;
    }
  }
  if (fresh || options.reindex) {
    // The index is of the base commit: no patch may be applied while it is built.
    git(clone, ["checkout", "-q", "--", "."]);
    git(clone, ["clean", "-fdq"]);
    rmSync(environment.GCAL_HOME, { recursive: true, force: true });
    for (const suffix of ["", "-wal", "-shm"]) rmSync(`${environment.GOLDENEYE_DB_PATH}${suffix}`, { force: true });
    const started = Date.now();
    const initialized = gcal(["init"]);
    console.log(`${task}: indexed at ${config.base_ref.slice(0, 8)} in ${Math.round((Date.now() - started) / 1000)} s${initialized.status === 0 ? "" : ` (failed: ${initialized.stderr.slice(0, 200)})`}`);
  }

  // With --types the repository's installed dependencies are linked into the
  // clone for the time of one check only. `git clean` and a recursive delete
  // would follow a link that is still there into the repository's own files.
  const dependencies = join(clone, "node_modules");
  const linkable = options.types && existsSync(join(repository, "node_modules")) && !existsSync(dependencies);

  const runsDirectory = join(repoRoot, "target", "agent-bench", task, "one-shot");
  const recorded = existsSync(runsDirectory) ? readdirSync(runsDirectory).filter((id) => attempts.test(id)).sort() : [];
  for (const attempt of recorded) {
    let run;
    try {
      run = JSON.parse(readFileSync(join(runsDirectory, attempt, "report.json"), "utf8")).runs?.[0];
    } catch {
      continue;
    }
    // A run the harness refused before any model call has nothing to replay.
    if (!run?.input_tokens) continue;
    const patches = find(join(runsDirectory, attempt), "patch.diff");
    if (patches.length === 0) continue;
    const patch = readFileSync(patches[0], "utf8");

    git(clone, ["checkout", "-q", "--", "."]);
    git(clone, ["clean", "-fdq"]);
    let applied = patch.trim().length === 0 ? "empty" : "not applied";
    const temporary = join(state, `${task}.patch`);
    // A patch recorded on a checkout with other line endings applies once converted.
    const unix = patch.replace(/\r\n/g, "\n");
    for (const [label, text] of patch.trim().length === 0 ? [] : [["as recorded", patch], ["lf", unix], ["crlf", unix.replace(/\n/g, "\r\n")]]) {
      writeFileSync(temporary, text);
      if (git(clone, ["apply", "--whitespace=nowarn", temporary]).status === 0) {
        applied = label;
        break;
      }
    }
    if (applied === "not applied") {
      console.log(`${task} ${attempt} grader=${run.success ? "pass" : "FAIL"} | patch does not apply; skipped`);
      continue;
    }

    let lines;
    if (linkable) symlinkSync(join(repository, "node_modules"), dependencies, "junction");
    try {
      lines = gcal(options.types ? ["check", "--types"] : ["check"]).stdout.split("\n");
    } finally {
      if (linkable) rmdirSync(dependencies);
    }
    const start = lines.findIndex((line) => REPORTED.test(line));
    const reported = start < 0 ? [] : lines.slice(start).filter((line) => line.trim().length > 0);
    totals.replayed += 1;
    if (!run.success) totals.failed += 1;
    if (reported.length > 0) totals[run.success ? "passedReported" : "failedReported"] += 1;
    console.log(`${task} ${attempt} grader=${run.success ? "pass" : "FAIL"} | ${lines[0]} | ${reported.length === 0 ? "nothing reported" : "reported:"}`);
    for (const line of reported.slice(0, 12)) console.log(`      ${line.slice(0, 200)}`);
  }
  git(clone, ["checkout", "-q", "--", "."]);
  git(clone, ["clean", "-fdq"]);
}
console.log(
  `replayed ${totals.replayed} runs: ${totals.failed} failed their grader, ${totals.failedReported} of those reported; ${totals.replayed - totals.failed} passed, ${totals.passedReported} of those reported`,
);
