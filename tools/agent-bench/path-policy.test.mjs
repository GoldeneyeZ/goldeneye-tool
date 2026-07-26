import assert from "node:assert/strict";
import test from "node:test";

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
      normalized: ["settings.gradle", "spring-core/A.java"],
      disallowed: ["settings.gradle"],
      missing_required_prefixes: [],
      below_minimum: false,
      above_maximum: false,
    },
  );
});
