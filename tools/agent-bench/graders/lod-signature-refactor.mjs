#!/usr/bin/env node
// Held-out grader for the `lod-signature-refactor` task.
//
//   node lod-signature-refactor.mjs <worktree> [<source repo>]
//
// Exit 0 = pass; failures are printed. The work is done by the companion
// script `fixtures/lod-signature-refactor.check.py`, which walks every `.py`
// file of the worktree with Python's `ast` module:
//   - no call, import, attribute or alias of the old `canonical` is left;
//   - `canonical_json(value)` is defined exactly once, in
//     `src/lod/training/evaluation.py`, with exactly that parameter list;
//   - every import of it comes from `lod.training.evaluation`, every bare call
//     has such an import in scope, every call passes exactly one argument;
//   - the number of new-form call sites is at least the number of old-form
//     call sites counted on the base commit (read with `git archive`);
//   - every Python file still parses and compiles;
//   - the moved function behaves as before, and 13 of the repository's own
//     unittest files (standard library only, a few seconds) still pass.
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const worktree = resolve(process.argv[2] ?? ".");
const here = dirname(fileURLToPath(import.meta.url));
const checker = join(here, "fixtures", "lod-signature-refactor.check.py");

if (!existsSync(worktree)) fatal(`worktree not found: ${worktree}`);
if (!existsSync(checker)) fatal(`companion script not found: ${checker}`);

const python = findPython();
if (!python) fatal("no Python >= 3.9 interpreter found (tried python, python3, py -3)");

const run = spawnSync(python.command, [...python.args, checker, worktree], {
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
