# Agent Bench Folder Isolation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:goal-driven-development with `goal-driven-bypass` (recommended), `goal-driven-gated`, superfastpowers:subagent-driven-development, or superfastpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Relocate every benchmark runtime entrypoint under `tools/agent-bench/` while keeping Goldeneye production Cargo packages bench-free.

**Architecture:** Use a hard relocation with no compatibility wrappers. A contract test enforces physical isolation and Rust-production separation; a private Node package defines stable local commands; all tracked callers migrate atomically to new paths.
**Plan Acronym:** ABFI

**Tech Stack:** Node.js ESM, `node:test`, Cargo metadata, Git

---

## File structure

### Create

- `tools/agent-bench/isolation.test.mjs` — permanent physical/package/Cargo-boundary contract.
- `tools/agent-bench/package.json` — private ESM package and local command surface.
- `tools/agent-bench/README.md` — ownership, entrypoints, inputs, outputs, production boundary.

### Move

- `tools/benchmark-agent-tasks.mjs` → `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- `tools/benchmark-competitors.mjs` → `tools/agent-bench/bin/benchmark-competitors.mjs`

### Modify

- `tools/agent-bench/core.test.mjs` — new agent-runner path, migrated with entrypoints.
- Tracked docs/plans containing either old entrypoint path — mechanical exact-path migration.

### Preserve

- All `crates/**` production code.
- Root `Cargo.toml` and Cargo package graph.
- Benchmark CLI arguments, env vars, outputs, scoring, and exit behavior.
- Untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md`.

---

### Task 1: Add failing isolation contract

<TASK-ID>ABFI-1</TASK-ID>

**Files:**

- Create: `tools/agent-bench/isolation.test.mjs`

- [ ] **Step 1: Write the failing test**

```javascript
import assert from "node:assert/strict";
import { access, readFile, readdir } from "node:fs/promises";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const BENCH_ROOT = fileURLToPath(new URL(".", import.meta.url));
const REPO_ROOT = path.resolve(BENCH_ROOT, "../..");
const BIN_ROOT = path.join(BENCH_ROOT, "bin");

const NEW_ENTRYPOINTS = [
  path.join(BIN_ROOT, "benchmark-agent-tasks.mjs"),
  path.join(BIN_ROOT, "benchmark-competitors.mjs"),
];

const OLD_ENTRYPOINTS = [
  path.join(REPO_ROOT, "tools", "benchmark-agent-tasks.mjs"),
  path.join(REPO_ROOT, "tools", "benchmark-competitors.mjs"),
];

async function exists(candidate) {
  try {
    await access(candidate);
    return true;
  }
  catch {
    return false;
  }
}

async function collectFiles(root) {
  const entries = await readdir(root, { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const candidate = path.join(root, entry.name);
    return entry.isDirectory() ? collectFiles(candidate) : [candidate];
  }));
  return nested.flat();
}

test("benchmark runtime entrypoints live only under tools/agent-bench", async () => {
  for (const entrypoint of NEW_ENTRYPOINTS) {
    assert.equal(await exists(entrypoint), true, `missing ${entrypoint}`);
  }
  for (const entrypoint of OLD_ENTRYPOINTS) {
    assert.equal(await exists(entrypoint), false, `legacy entrypoint remains: ${entrypoint}`);
  }
});

test("entrypoint relative imports remain inside tools/agent-bench", async () => {
  for (const entrypoint of NEW_ENTRYPOINTS) {
    const source = await readFile(entrypoint, "utf8");
    const imports = [...source.matchAll(
      /(?:from\s*|import\s*\()\s*["']([^"']+)["']/g,
    )].map((match) => match[1]);

    for (const specifier of imports.filter((value) => value.startsWith("."))) {
      const resolved = path.resolve(path.dirname(entrypoint), specifier);
      assert.equal(
        path.relative(BENCH_ROOT, resolved).startsWith(".."),
        false,
        `${entrypoint} escapes bench root through ${specifier}`,
      );
    }
  }
});

test("production Rust sources do not reference benchmark runtime paths", async () => {
  const candidates = [
    path.join(REPO_ROOT, "Cargo.toml"),
    ...(await collectFiles(path.join(REPO_ROOT, "crates")))
      .filter((candidate) => /\.(?:rs|toml)$/.test(candidate)),
  ];
  const forbidden = /tools[\\/]agent-bench|benchmark-agent-tasks|benchmark-competitors/;

  for (const candidate of candidates) {
    const source = await readFile(candidate, "utf8");
    assert.doesNotMatch(source, forbidden, candidate);
  }
});
```

- [ ] **Step 2: Run test to verify RED**

Run:

```powershell
node --test tools/agent-bench/isolation.test.mjs
```

Expected: FAIL because new `bin/` entrypoints are missing and legacy top-level entrypoints still exist. Production Rust boundary test passes.

- [ ] **Step 3: Commit failing contract**

```powershell
git add -- tools/agent-bench/isolation.test.mjs
git commit -m "test(bench): require one-folder runtime isolation"
```

---

### Task 2: Relocate benchmark entrypoints

<TASK-ID>ABFI-2</TASK-ID>

**Files:**

- Move: `tools/benchmark-agent-tasks.mjs` → `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Move: `tools/benchmark-competitors.mjs` → `tools/agent-bench/bin/benchmark-competitors.mjs`
- Modify: `tools/agent-bench/core.test.mjs`
- Test: `tools/agent-bench/isolation.test.mjs`

- [ ] **Step 1: Move agent-task runner and rewrite its local imports**

Move file through `apply_patch`, preserving all content except these import rewrites:

```javascript
} from "../core.mjs";
import { evaluateDirtyPathPolicy } from "../path-policy.mjs";
import { prepareCleanSnapshot } from "../snapshot.mjs";
import { buildTimingBreakdown } from "../timing.mjs";
import {
  captureBenchmarkProvenance,
  diffBenchmarkProvenance,
} from "../provenance.mjs";
import { buildBenchmarkReport } from "../report.mjs";
import { evaluateQualification } from "../qualification.mjs";
```

Every previous `./agent-bench/<module>.mjs` import becomes `../<module>.mjs`. No other code changes.

- [ ] **Step 2: Move competitor runner unchanged**

Move through `apply_patch`:

```text
tools/benchmark-competitors.mjs
→ tools/agent-bench/bin/benchmark-competitors.mjs
```

Its imports are Node built-ins; no relative rewrite is required.

- [ ] **Step 3: Update direct test reference**

In `tools/agent-bench/core.test.mjs`, replace:

```text
tools/benchmark-agent-tasks.mjs
```

with:

```text
tools/agent-bench/bin/benchmark-agent-tasks.mjs
```

- [ ] **Step 4: Run syntax checks**

```powershell
node --check tools/agent-bench/bin/benchmark-agent-tasks.mjs
node --check tools/agent-bench/bin/benchmark-competitors.mjs
```

Expected: both exit `0`, no output.

- [ ] **Step 5: Run isolation and affected tests to verify GREEN**

```powershell
node --test tools/agent-bench/isolation.test.mjs
node --test tools/agent-bench/core.test.mjs
```

Expected: both test files pass, 0 fail.

- [ ] **Step 6: Commit relocation**

```powershell
git add -- tools/benchmark-agent-tasks.mjs tools/benchmark-competitors.mjs tools/agent-bench/bin tools/agent-bench/core.test.mjs
git commit -m "refactor(bench): isolate runtime entrypoints"
```

---

### Task 3: Add private package boundary and operator README

<TASK-ID>ABFI-3</TASK-ID>

**Files:**

- Modify: `tools/agent-bench/isolation.test.mjs`
- Create: `tools/agent-bench/package.json`
- Create: `tools/agent-bench/README.md`

- [ ] **Step 1: Add failing package-contract test**

Append to `tools/agent-bench/isolation.test.mjs`:

```javascript
test("bench package is private ESM with stable commands", async () => {
  const manifest = JSON.parse(
    await readFile(path.join(BENCH_ROOT, "package.json"), "utf8"),
  );

  assert.equal(manifest.private, true);
  assert.equal(manifest.type, "module");
  assert.equal(manifest.engines.node, ">=20");
  assert.deepEqual(manifest.scripts, {
    test: "node --test",
    check: "node --check ./bin/benchmark-agent-tasks.mjs && node --check ./bin/benchmark-competitors.mjs",
    "benchmark:agents": "node ./bin/benchmark-agent-tasks.mjs",
    "benchmark:competitors": "node ./bin/benchmark-competitors.mjs",
  });
});
```

- [ ] **Step 2: Run test to verify RED**

```powershell
node --test tools/agent-bench/isolation.test.mjs
```

Expected: FAIL with `ENOENT` for `tools/agent-bench/package.json`.

- [ ] **Step 3: Add package manifest**

Create `tools/agent-bench/package.json`:

```json
{
  "name": "@goldeneye/agent-bench",
  "private": true,
  "type": "module",
  "engines": {
    "node": ">=20"
  },
  "scripts": {
    "test": "node --test",
    "check": "node --check ./bin/benchmark-agent-tasks.mjs && node --check ./bin/benchmark-competitors.mjs",
    "benchmark:agents": "node ./bin/benchmark-agent-tasks.mjs",
    "benchmark:competitors": "node ./bin/benchmark-competitors.mjs"
  }
}
```

- [ ] **Step 4: Add README**

Create `tools/agent-bench/README.md` with:

```markdown
# Goldeneye agent benchmark tools

All benchmark runtime code, configs, tasks, graders, and fixtures live in this directory. Production Cargo packages do not import this directory.

## Requirements

- Node.js 20 or newer
- Benchmark-specific external repositories, commands, and environment variables required by the selected config

## Commands

From repository root:

```powershell
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
npm --prefix tools/agent-bench run benchmark:agents -- <arguments>
npm --prefix tools/agent-bench run benchmark:competitors -- <arguments>
```

Direct entrypoints:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs <arguments>
node tools/agent-bench/bin/benchmark-competitors.mjs <arguments>
```

## Layout

- `bin/`: benchmark entrypoints
- `configs/`: benchmark configurations
- `graders/`: grading code and injected fixtures
- `tasks/`: agent task prompts
- root modules: shared harness, timing, provenance, qualification, reporting, and snapshot logic

## Inputs and outputs

Entrypoint arguments and environment variables remain defined by each runner and config. Generated benchmark evidence belongs under configured output directories, normally `target/agent-bench/`; generated evidence is not production runtime input.

## Production boundary

Goldeneye production builds use Cargo packages under `crates/`. Removing `tools/agent-bench/` removes benchmark runtime tooling without changing the Cargo package graph. Legacy paths `tools/benchmark-agent-tasks.mjs` and `tools/benchmark-competitors.mjs` no longer exist.
```

- [ ] **Step 5: Verify GREEN**

```powershell
node --test tools/agent-bench/isolation.test.mjs
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
```

Expected: all tests pass; both syntax checks exit `0`.

- [ ] **Step 6: Commit package boundary**

```powershell
git add -- tools/agent-bench/isolation.test.mjs tools/agent-bench/package.json tools/agent-bench/README.md
git commit -m "docs(bench): define isolated package boundary"
```

---

### Task 4: Migrate every tracked entrypoint reference

<TASK-ID>ABFI-4</TASK-ID>

**Files:**

- Modify: `docs/abyssal-zenith-serena-benchmark.md`
- Modify: `docs/agent-task-benchmark.md`
- Modify: `docs/goldeneye-stability-round-1.md`
- Modify: `docs/benchmarks/2026-07-14-codebase-memory-vs-goldeneye.md`
- Modify: `docs/benchmarks/PERFORMANCE-OPTIMIZATION-PROMPT.md`
- Modify: `FOLDER_STRUCTURE.md`
- Modify: tracked files under:
  - `docs/superfastpowers/plans/SSRB/2026-07-25-spring-sensitive-redaction-million-token-benchmark/`
  - `docs/superfastpowers/plans/SUWB/2026-07-24-spring-unicode-warm-benchmark/`

- [ ] **Step 1: Apply exact mechanical replacements**

Across tracked files:

```text
tools/benchmark-agent-tasks.mjs
→ tools/agent-bench/bin/benchmark-agent-tasks.mjs

tools/benchmark-competitors.mjs
→ tools/agent-bench/bin/benchmark-competitors.mjs
```

Use this one-time Node rewrite from repository root; it preserves existing line endings and changes only exact strings:

```powershell
@'
const { execFileSync } = require("node:child_process");
const { readFileSync, writeFileSync } = require("node:fs");

const replacements = new Map([
  ["tools/benchmark-agent-tasks.mjs", "tools/agent-bench/bin/benchmark-agent-tasks.mjs"],
  ["tools/benchmark-competitors.mjs", "tools/agent-bench/bin/benchmark-competitors.mjs"],
]);

const files = execFileSync("git", ["grep", "-Il", "-e", [...replacements.keys()][0], "-e", [...replacements.keys()][1]], {
  encoding: "utf8",
}).split(/\r?\n/).filter(Boolean);

for (const file of files) {
  const before = readFileSync(file, "utf8");
  let after = before;
  for (const [from, to] of replacements) {
    after = after.replaceAll(from, to);
  }
  if (after !== before) {
    writeFileSync(file, after);
  }
}
'@ | node
```

- [ ] **Step 2: Verify old references are gone**

```powershell
$oldAgent = git grep -n -- 'tools/benchmark-agent-tasks.mjs'
$oldCompetitor = git grep -n -- 'tools/benchmark-competitors.mjs'
if ($oldAgent -or $oldCompetitor) {
  throw "legacy benchmark entrypoint reference remains"
}
```

Expected: no matches and no exception.

- [ ] **Step 3: Run affected tests**

```powershell
node --test tools/agent-bench/core.test.mjs
node --test tools/agent-bench/isolation.test.mjs
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
```

Expected: all tests and checks pass.

- [ ] **Step 4: Review path-only diff**

```powershell
git diff --word-diff=porcelain -- FOLDER_STRUCTURE.md docs tools/agent-bench/core.test.mjs
```

Expected: entrypoint-reference edits only, excluding new package/README/test files committed in prior tasks.

- [ ] **Step 5: Commit caller migration**

```powershell
git add -u -- FOLDER_STRUCTURE.md docs
git commit -m "docs(bench): migrate isolated entrypoint paths"
```

Do not add untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md`.

---

### Task 5: Verify production separation and full acceptance

<TASK-ID>ABFI-5</TASK-ID>

**Files:**

- Verify only; no planned modifications.

- [ ] **Step 1: Run complete bench test package**

```powershell
npm --prefix tools/agent-bench test
npm --prefix tools/agent-bench run check
```

Expected: all tests pass; checks exit `0`.

- [ ] **Step 2: Verify physical one-folder isolation**

```powershell
$outside = git ls-files tools | Where-Object {
  $_ -match '(?i)bench' -and $_ -notlike 'tools/agent-bench/*'
}
if ($outside) {
  $outside
  throw "benchmark runtime file remains outside tools/agent-bench"
}
```

Expected: no output and no exception.

- [ ] **Step 3: Verify Cargo package graph**

```powershell
$metadata = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
$toolRefs = @($metadata.packages | Where-Object {
  $_.manifest_path -match 'tools[\\/]agent-bench|benchmark-agent|benchmark-competitors' -or
  @($_.targets | Where-Object {
    $_.src_path -match 'tools[\\/]agent-bench|benchmark-agent|benchmark-competitors'
  }).Count -gt 0
})
if ($toolRefs.Count -ne 0) {
  $toolRefs | ConvertTo-Json -Depth 10
  throw "Cargo package graph references benchmark tooling"
}
```

Expected: zero matching packages and no exception.

- [ ] **Step 4: Verify repository hygiene**

```powershell
git diff --check
git status --short
```

Expected:

- `git diff --check` exits `0`.
- Only pre-existing untracked `docs/benchmarks/lane1-r6-call-dependency-tree.md` may remain.

- [ ] **Step 5: Record final evidence**

Record:

- Node test count and failures.
- Syntax-check status.
- Old-path grep count.
- Runtime files outside `tools/agent-bench/`.
- Cargo tool-reference count.
- Final commits for ABFI-1 through ABFI-4.
