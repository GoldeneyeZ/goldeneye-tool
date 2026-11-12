#!/usr/bin/env node
// Grader for the `pyfinance-behavior-bug` task.
//
//   node pyfinance-behavior-bug.mjs <worktree> [<source repo>]
//
// 1. The agent must have added or changed at least one Python test file that
//    mentions a timezone / UTC offset.
// 2. A held-out test file is copied into `<worktree>/tests`, run offline and
//    removed again. It is run with `python -m pytest` when pytest is importable
//    and as a plain unittest script otherwise (the file supports both).
//
// Exit 0 = pass. Failures are printed to stderr.
import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const BASE_COMMIT = "24aae900a8a2bcb9e0f788a3db1b3226ae58011c";
const HELD_OUT_REL = "tests/test_agent_bench_heldout_price_dates.py";

const worktree = resolve(process.argv[2] ?? ".");
const here = dirname(fileURLToPath(import.meta.url));
const fixturePath = join(here, "fixtures", "pyfinance-behavior-bug.heldout_test.py");
const heldOutPath = join(worktree, ...HELD_OUT_REL.split("/"));
const python = process.env.AGENT_BENCH_PYTHON || "python";

const failures = [];

if (!existsSync(join(worktree, "src", "pyfinance"))) {
  failures.push(`not a PyFinance worktree: ${worktree}`);
} else {
  checkAgentTests();
  runHeldOutTests();
}

if (failures.length > 0) {
  console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
  process.exit(1);
}
console.log("PASS: offset-carrying price dates keep their calendar day; regression test present");

function checkAgentTests() {
  const changed = new Set();
  let tracked = git(["diff", "--name-only", BASE_COMMIT]);
  if (tracked.status !== 0) tracked = git(["diff", "--name-only", "HEAD"]);
  const untracked = git(["ls-files", "--others", "--exclude-standard"]);
  for (const result of [tracked, untracked]) {
    for (const line of (result.stdout ?? "").split(/\r?\n/)) {
      const path = line.trim().replaceAll("\\", "/");
      if (path) changed.add(path);
    }
  }
  const testFiles = [...changed].filter(
    (path) =>
      path !== HELD_OUT_REL &&
      /(^|\/)(test_[^/]*|[^/]*_test)\.py$/.test(path) &&
      existsSync(join(worktree, ...path.split("/"))),
  );
  if (testFiles.length === 0) {
    failures.push("no Python test file was added or changed (a regression test is required)");
    return;
  }
  const mentionsOffset = testFiles.some((path) =>
    /\d[+-]\d{2}:\d{2}\b|tz_|tzinfo|timezone|utc|offset|ZoneInfo/i.test(
      readFileSync(join(worktree, ...path.split("/")), "utf8"),
    ),
  );
  if (!mentionsOffset) {
    failures.push(
      `changed test files (${testFiles.join(", ")}) do not exercise dates with a UTC offset or timezone`,
    );
  }
}

function runHeldOutTests() {
  const original = existsSync(heldOutPath) ? readFileSync(heldOutPath) : null;
  const env = {
    ...process.env,
    PYTHONPATH: [join(worktree, "src"), process.env.PYTHONPATH].filter(Boolean).join(delimiter),
    PYTHONDONTWRITEBYTECODE: "1",
    PYTHONIOENCODING: "utf-8",
  };
  const options = {
    cwd: worktree,
    env,
    encoding: "utf8",
    maxBuffer: 10 * 1024 * 1024,
    timeout: 150_000,
    windowsHide: true,
  };
  try {
    writeFileSync(heldOutPath, readFileSync(fixturePath));
    const probe = spawnSync(python, ["-c", "import pytest"], { ...options, timeout: 60_000 });
    if (probe.error) {
      failures.push(`cannot start Python interpreter "${python}": ${probe.error.message}`);
      return;
    }
    const args =
      probe.status === 0
        ? ["-m", "pytest", HELD_OUT_REL, "-q", "-p", "no:cacheprovider"]
        : [HELD_OUT_REL];
    const run = spawnSync(python, args, options);
    if (run.error) {
      failures.push(`held-out tests did not complete: ${run.error.message}`);
    } else if (run.status !== 0) {
      const runner = probe.status === 0 ? "pytest" : "unittest";
      failures.push(
        `held-out tests failed (${runner}, exit ${run.status}):\n${tail(`${run.stdout ?? ""}\n${run.stderr ?? ""}`)}`,
      );
    }
  } finally {
    if (original === null) rmSync(heldOutPath, { force: true });
    else writeFileSync(heldOutPath, original);
  }
}

function git(args) {
  return spawnSync("git", ["-C", worktree, ...args], {
    encoding: "utf8",
    maxBuffer: 10 * 1024 * 1024,
    windowsHide: true,
  });
}

function tail(text, lines = 60) {
  return text.trim().split(/\r?\n/).slice(-lines).join("\n");
}
