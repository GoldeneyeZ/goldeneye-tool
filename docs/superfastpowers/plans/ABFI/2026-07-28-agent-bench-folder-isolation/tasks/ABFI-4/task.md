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
tools/agent-bench/bin/benchmark-agent-tasks.mjs
→ tools/agent-bench/bin/benchmark-agent-tasks.mjs

tools/agent-bench/bin/benchmark-competitors.mjs
→ tools/agent-bench/bin/benchmark-competitors.mjs
```

Use this one-time Node rewrite from repository root; it preserves existing line endings and changes only exact strings:

```powershell
@'
const { execFileSync } = require("node:child_process");
const { readFileSync, writeFileSync } = require("node:fs");

const replacements = new Map([
  ["tools/agent-bench/bin/benchmark-agent-tasks.mjs", "tools/agent-bench/bin/benchmark-agent-tasks.mjs"],
  ["tools/agent-bench/bin/benchmark-competitors.mjs", "tools/agent-bench/bin/benchmark-competitors.mjs"],
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
$oldAgent = git grep -n -- 'tools/agent-bench/bin/benchmark-agent-tasks.mjs'
$oldCompetitor = git grep -n -- 'tools/agent-bench/bin/benchmark-competitors.mjs'
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
