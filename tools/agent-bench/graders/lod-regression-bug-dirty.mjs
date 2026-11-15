#!/usr/bin/env node
// Held-out grader for the `lod-regression-bug-dirty` task.
//
//   node lod-regression-bug-dirty.mjs <worktree> [<source repo>]
//
// Exit 0 = pass; failures are printed. The work is done by the companion
// script `fixtures/lod-regression-bug-dirty.check.py` (standard library only):
//   - `tests/datasets/test_expansion_combine.py` is the base commit's file;
//   - the rest of the uncommitted work of the overlay is still there (the new
//     `naming.py`, the helpers and renames in `build.py`, `combine.py`,
//     `dense_labels.py`, `opaque.py`, `varied.py`), read with `ast`;
//   - the expansion package behaves as on the base commit: the base sources
//     are read with `git archive` into a temporary directory and one probe is
//     run against both trees (generated rows for several seeds, manifests, a
//     combined corpus, rejection of repeated targets);
//   - the base commit's expansion tests (22, a few seconds) pass against the
//     worktree sources, run from the temporary directory.
// Nothing is written into the worktree.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const worktree = resolve(process.argv[2] ?? ".");
const sourceRepo = process.argv[3] ? resolve(process.argv[3]) : "";
const here = dirname(fileURLToPath(import.meta.url));
const checker = join(here, "fixtures", "lod-regression-bug-dirty.check.py");

if (!existsSync(worktree)) fatal(`worktree not found: ${worktree}`);
if (!existsSync(checker)) fatal(`companion script not found: ${checker}`);

const python = findPython();
if (!python) fatal("no Python >= 3.9 interpreter found (tried python, python3, py -3)");

const args = [...python.args, checker, worktree];
if (sourceRepo && existsSync(sourceRepo)) args.push(sourceRepo);
const run = spawnSync(python.command, args, {
  cwd: worktree,
  encoding: "utf8",
  env: { ...process.env, PYTHONIOENCODING: "utf-8", PYTHONDONTWRITEBYTECODE: "1" },
  maxBuffer: 16 * 1024 * 1024,
  timeout: 170_000,
  windowsHide: true,
});

if (run.error) fatal(`could not run the Python checker: ${run.error.message}`);
const output = `${run.stdout ?? ""}${run.stderr ? `\n${tail(run.stderr)}` : ""}`.trim();
if (run.status !== 0) {
  console.error(output || `Python checker exited with status ${run.status}`);
  process.exit(1);
}
console.log(output);

function findPython() {
  const candidates = [
    ...(process.env.AGENT_BENCH_PYTHON ? [{ command: process.env.AGENT_BENCH_PYTHON, args: [] }] : []),
    { command: "python", args: [] },
    { command: "python3", args: [] },
    { command: "py", args: ["-3"] },
  ];
  for (const candidate of candidates) {
    const probe = spawnSync(
      candidate.command,
      [...candidate.args, "-c", "import sys; print('%d.%d' % sys.version_info[:2])"],
      { encoding: "utf8", timeout: 20_000, windowsHide: true },
    );
    if (probe.status !== 0 || !probe.stdout) continue;
    const [major, minor] = probe.stdout.trim().split(".").map(Number);
    if (major === 3 && minor >= 9) return candidate;
  }
  return null;
}

function tail(text) {
  return String(text).trim().split(/\r?\n/).slice(-25).join("\n");
}

function fatal(message) {
  console.error(message);
  process.exit(1);
}
