#!/usr/bin/env node
import { spawnSync } from "node:child_process";
import {
  existsSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { join, resolve } from "node:path";

const worktree = resolve(process.argv[2] ?? ".");
const repo = resolve(process.argv[3] ?? worktree);
const fuzzyPath = join(worktree, "src", "modules", "command-palette", "lib", "fuzzy.ts");
const testPath = join(worktree, "src", "modules", "command-palette", "lib", "fuzzy.test.ts");
const heldOutPath = join(
  worktree,
  "src",
  "modules",
  "command-palette",
  "lib",
  "agent-bench-fuzzy-diacritics.test.ts",
);
const nodeModules = join(worktree, "node_modules");
const sourceNodeModules = join(repo, "node_modules");
const vitest = join(sourceNodeModules, "vitest", "vitest.mjs");

const failures = [];
const fuzzy = readFileSync(fuzzyPath, "utf8");
const tests = readFileSync(testPath, "utf8");

requireMatch(
  fuzzy,
  /export\s+function\s+fuzzyScore\s*\(\s*query\s*:\s*string\s*,\s*target\s*:\s*string\s*\)\s*:\s*number\s*\|\s*null/,
  "fuzzyScore public signature changed",
);
requireMatch(
  fuzzy,
  /export\s+function\s+fuzzyBest\s*\(\s*query\s*:\s*string\s*,\s*candidates\s*:\s*string\[\]\s*\)\s*:\s*number\s*\|\s*null/,
  "fuzzyBest public signature changed",
);
requireMatch(tests, /accent|diacritic|Café|résumé|Ångström/i, "focused accent tests were not added");

const changed = git(["diff", "--name-only"])
  .split(/\r?\n/)
  .filter(Boolean);
if (changed.some((path) => path.endsWith(".rs"))) failures.push("Rust files must not be modified");

if (!existsSync(vitest)) {
  failures.push(`Vitest runtime not found at ${vitest}`);
} else if (failures.length === 0) {
  runHeldOutTests();
}

const diffCheck = spawnSync("git", ["-C", worktree, "diff", "--check"], {
  encoding: "utf8",
  windowsHide: true,
});
if (diffCheck.status !== 0) {
  failures.push(`git diff --check failed:\n${tail(diffCheck.stderr || diffCheck.stdout)}`);
}

if (failures.length > 0) {
  console.error(failures.map((failure, index) => `${index + 1}. ${failure}`).join("\n"));
  process.exit(1);
}
console.log("PASS: accent-insensitive fuzzy matching, regression tests, and frontend behavior verified");

function runHeldOutTests() {
  const original = existsSync(heldOutPath) ? readFileSync(heldOutPath) : null;
  let linkedNodeModules = false;
  const heldOut = `import { describe, expect, it } from "vitest";
import { fuzzyBest, fuzzyScore } from "./fuzzy";

describe("agent benchmark held-out accent matching", () => {
  it.each([
    ["cafe", "Café"],
    ["résumé", "resume"],
    ["angstrom", "Ångström"],
    ["cafe", "Cafe\\u0301"],
    ["é", "e\\u0301"],
  ])("matches %s against %s", (query, target) => {
    expect(fuzzyScore(query, target)).not.toBeNull();
  });

  it("retains existing ranking and miss behavior", () => {
    expect(fuzzyScore("np", "new private")).toBeGreaterThan(
      fuzzyScore("np", "unzip"),
    );
    expect(fuzzyScore("xyz", "Café settings")).toBeNull();
    expect(fuzzyBest("resume", ["close tab", "Résumé preview"])).not.toBeNull();
  });
});
`;
  try {
    writeFileSync(heldOutPath, heldOut);
    if (!existsSync(nodeModules)) {
      symlinkSync(sourceNodeModules, nodeModules, process.platform === "win32" ? "junction" : "dir");
      linkedNodeModules = true;
    }
    const run = spawnSync(
      process.execPath,
      [
        vitest,
        "run",
        "--root",
        worktree,
        "src/modules/command-palette/lib/fuzzy.test.ts",
        "src/modules/command-palette/lib/agent-bench-fuzzy-diacritics.test.ts",
      ],
      {
        cwd: worktree,
        encoding: "utf8",
        maxBuffer: 10 * 1024 * 1024,
        timeout: 120_000,
        windowsHide: true,
      },
    );
    if (run.status !== 0) {
      failures.push(`held-out frontend tests failed:\n${tail(run.stderr || run.stdout)}`);
    }
  } finally {
    if (original === null) rmSync(heldOutPath, { force: true });
    else writeFileSync(heldOutPath, original);
    if (linkedNodeModules) rmSync(nodeModules, { force: true, recursive: true });
  }
}

function git(args) {
  const result = spawnSync("git", ["-C", worktree, ...args], {
    encoding: "utf8",
    windowsHide: true,
  });
  if (result.status !== 0) failures.push(`git ${args.join(" ")} failed: ${tail(result.stderr)}`);
  return result.stdout ?? "";
}

function requireMatch(source, pattern, message) {
  if (!pattern.test(source)) failures.push(message);
}

function tail(value) {
  return String(value ?? "").slice(-5000);
}
