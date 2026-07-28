## Task 1: Add reusable dirty-path policies

<TASK-ID>SSRB-1</TASK-ID>

**Files:**
- Create: `tools/agent-bench/path-policy.mjs`
- Create: `tools/agent-bench/path-policy.test.mjs`
- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`

- [ ] **Step 1: Write failing normalization and policy tests**

```javascript
import test from "node:test";
import assert from "node:assert/strict";
import {
  compileDirtyPathPolicy,
  evaluateDirtyPaths,
  normalizeRepoPath,
} from "./path-policy.mjs";

test("normalizes repository-relative paths", () => {
  assert.equal(normalizeRepoPath(".\\spring-core\\src\\A.java"), "spring-core/src/A.java");
  assert.throws(() => normalizeRepoPath("../outside.java"), /repository-relative/);
  assert.throws(() => normalizeRepoPath("C:\\outside.java"), /repository-relative/);
});

test("accepts exact, prefix, and glob rules", () => {
  const policy = compileDirtyPathPolicy({
    exact: ["README.md"],
    prefixes: ["spring-context/src/main/java/"],
    globs: ["spring-web*/src/test/java/**/*.java"],
    min_paths: 2,
    max_paths: 8,
    required_prefixes: ["spring-context/src/main/java/"],
  });
  const result = evaluateDirtyPaths([
    "README.md",
    "spring-context/src/main/java/org/example/A.java",
    "spring-webmvc/src/test/java/org/example/ATests.java",
  ], policy);
  assert.equal(result.passed, true);
  assert.deepEqual(result.disallowed, []);
  assert.deepEqual(result.missing_required_prefixes, []);
});

test("reports disallowed paths and cardinality failures", () => {
  const policy = compileDirtyPathPolicy({
    prefixes: ["spring-core/"],
    min_paths: 2,
    max_paths: 3,
  });
  assert.deepEqual(
    evaluateDirtyPaths(["spring-core/A.java", "settings.gradle"], policy),
    {
      passed: false,
      normalized: ["spring-core/A.java", "settings.gradle"],
      disallowed: ["settings.gradle"],
      missing_required_prefixes: [],
      below_minimum: false,
      above_maximum: false,
    },
  );
});
```

- [ ] **Step 2: Run tests and verify the missing-module failure**

Run:

```powershell
node --test tools/agent-bench/path-policy.test.mjs
```

Expected: FAIL with `ERR_MODULE_NOT_FOUND` for `path-policy.mjs`.

- [ ] **Step 3: Implement path normalization and policy evaluation**

```javascript
const WINDOWS_ABSOLUTE = /^[A-Za-z]:[\\/]/;

export function normalizeRepoPath(value) {
  const normalized = String(value).replaceAll("\\", "/").replace(/^\.\/+/, "");
  if (!normalized || normalized.startsWith("/") || WINDOWS_ABSOLUTE.test(normalized) ||
      normalized.split("/").includes("..")) {
    throw new Error(`Dirty path must be repository-relative: ${value}`);
  }
  return normalized;
}

export function compileDirtyPathPolicy(config = {}) {
  const exact = new Set((config.exact ?? []).map(normalizeRepoPath));
  const prefixes = (config.prefixes ?? []).map(normalizeRepoPath)
    .map((value) => value.endsWith("/") ? value : `${value}/`);
  const globs = (config.globs ?? []).map((value) => ({
    source: normalizeRepoPath(value),
    regex: globToRegExp(normalizeRepoPath(value)),
  }));
  return {
    exact,
    prefixes,
    globs,
    min_paths: config.min_paths ?? 0,
    max_paths: config.max_paths ?? Number.POSITIVE_INFINITY,
    required_prefixes: (config.required_prefixes ?? []).map(normalizeRepoPath)
      .map((value) => value.endsWith("/") ? value : `${value}/`),
  };
}

export function evaluateDirtyPaths(paths, policy) {
  const normalized = [...new Set(paths.map(normalizeRepoPath))].sort();
  const allowed = (path) => policy.exact.has(path) ||
    policy.prefixes.some((prefix) => path.startsWith(prefix)) ||
    policy.globs.some((glob) => glob.regex.test(path));
  const disallowed = normalized.filter((path) => !allowed(path));
  const missingRequiredPrefixes = policy.required_prefixes
    .filter((prefix) => !normalized.some((path) => path.startsWith(prefix)));
  const belowMinimum = normalized.length < policy.min_paths;
  const aboveMaximum = normalized.length > policy.max_paths;
  return {
    passed: disallowed.length === 0 && missingRequiredPrefixes.length === 0 &&
      !belowMinimum && !aboveMaximum,
    normalized,
    disallowed,
    missing_required_prefixes: missingRequiredPrefixes,
    below_minimum: belowMinimum,
    above_maximum: aboveMaximum,
  };
}

function globToRegExp(glob) {
  let source = "";
  for (let index = 0; index < glob.length; index += 1) {
    const char = glob[index];
    if (char === "*" && glob[index + 1] === "*") {
      source += ".*";
      index += 1;
    }
    else if (char === "*") source += "[^/]*";
    else if (char === "?") source += "[^/]";
    else source += char.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
  }
  return new RegExp(`^${source}$`);
}
```

- [ ] **Step 4: Wire policy evaluation into run finalization**

Load `config.allowed_dirty_policy`, fall back to
`config.allowed_dirty_paths`, and persist the complete result:

```javascript
const dirtyPolicy = compileDirtyPathPolicy(
  config.allowed_dirty_policy ?? { exact: config.allowed_dirty_paths ?? [] },
);
const dirtyEvaluation = evaluateDirtyPaths(dirtyFileNames, dirtyPolicy);
result.dirty_path_policy = dirtyEvaluation;
if (!dirtyEvaluation.passed) {
  result.success = false;
  result.protocol_violations.push({
    kind: "dirty-path-policy",
    ...dirtyEvaluation,
  });
}
```

- [ ] **Step 5: Run focused and full harness tests**

Run:

```powershell
node --test tools/agent-bench/path-policy.test.mjs
node --test tools/agent-bench/*.test.mjs
```

Expected: all tests PASS.

- [ ] **Step 6: Commit**

```powershell
git add -- tools/agent-bench/path-policy.mjs tools/agent-bench/path-policy.test.mjs tools/agent-bench/bin/benchmark-agent-tasks.mjs
git commit -m "bench: add dirty path policies"
```
