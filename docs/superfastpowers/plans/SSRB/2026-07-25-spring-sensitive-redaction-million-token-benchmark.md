# Spring Sensitive Redaction Benchmark Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superfastpowers:goal-driven-development with `goal-driven-bypass` (recommended), `goal-driven-gated`, superfastpowers:subagent-driven-development, or superfastpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build and execute an audited Spring Framework benchmark whose clean vanilla lane has a three-run median of `800,000–1,200,000` cumulative input tokens and at least `100,000` uncached input tokens.

**Architecture:** Extend the existing agent harness with reusable dirty-path policies, variable lane-count audits, sample variability statistics, and non-scored calibration gates. Add a versioned Spring sensitive-value redaction task with hidden multi-module tests, build an immutable six-module ACK snapshot, calibrate through a predeclared complexity ladder, then freeze and run one randomized clean `3 + 3` comparison.
**Plan Acronym:** SSRB


**Tech Stack:** Node.js 24 ESM, `node:test`, PowerShell 7, Rust/Goldeneye release CLI, ACK, Codex CLI, Java 17, Gradle 8, Spring Framework

---

## File Structure

### Harness policy and statistics

- Create `tools/agent-bench/path-policy.mjs`: normalize repository paths and
  evaluate exact, prefix, and glob dirty-path policies.
- Create `tools/agent-bench/path-policy.test.mjs`: unit tests for traversal,
  separators, globs, prefixes, and cardinality.
- Create `tools/agent-bench/qualification.mjs`: calibration and scored-lane
  token gates.
- Create `tools/agent-bench/qualification.test.mjs`: qualification boundary
  and failure-reason tests.
- Modify `tools/agent-bench/core.mjs`: sample SD and coefficient-of-variation
  summary fields.
- Modify `tools/agent-bench/core.test.mjs`: summary-statistic tests.
- Modify `tools/agent-bench/report.mjs`: dynamic lane counts, dynamic
  limitations, policy-based dirty-path audit.
- Modify `tools/agent-bench/report.test.mjs`: 3+1 compatibility and 3+3 audit
  tests.
- Modify `tools/agent-bench/bin/benchmark-agent-tasks.mjs`: calibration CLI, policy wiring,
  qualification persistence, dynamic audit options.

### Spring task and grader

- Create `tools/agent-bench/tasks/spring-sensitive-value-redaction-level2.md`:
  agent-visible production task.
- Create `tools/agent-bench/graders/spring-sensitive-value-redaction.ps1`:
  hidden fixture installation, focused Gradle execution, cleanup, and protocol
  checks.
- Create
  `tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1`:
  grader contract tests.
- Create
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-core/SensitiveAnnotationAgentBenchTests.java`:
  marker contract tests.
- Create
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-context/SensitiveDataBinderAgentBenchTests.java`:
  binding and validation tests.
- Create
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-context/SensitiveMethodValidationAgentBenchTests.java`:
  method-validation tests.
- Create
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-web/SensitiveWebBindingInitializerAgentBenchTests.java`:
  initializer propagation tests.
- Create
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-webmvc/SensitiveMvcAgentBenchTests.java`:
  MVC integration tests.
- Create
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-webflux/SensitiveWebFluxAgentBenchTests.java`:
  WebFlux integration tests.

### Configuration and artifacts

- Create
  `tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json`:
  pinned repo, six-module ACK snapshot, lane policy, token gates, and grader.
- Generate
  `target/agent-bench/snapshots/spring-sensitive-value-redaction-level2/**`:
  immutable ACK snapshot.
- Generate
  `target/agent-bench/spring-sensitive-value-redaction-level2/calibration/**`:
  versioned non-scored vanilla pilots.
- Generate
  `target/agent-bench/spring-sensitive-value-redaction-level2/scored/**`:
  six scored raw artifact directories.
- Generate
  `target/agent-bench/spring-sensitive-value-redaction-level2/report.json`:
  audited raw report.
- Generate
  `target/agent-bench/spring-sensitive-value-redaction-level2/report.md`:
  corrected user-facing analysis.

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

## Task 2: Generalize report audit and variability statistics

<TASK-ID>SSRB-2</TASK-ID>

**Files:**
- Modify: `tools/agent-bench/core.mjs`
- Modify: `tools/agent-bench/core.test.mjs`
- Modify: `tools/agent-bench/report.mjs`
- Modify: `tools/agent-bench/report.test.mjs`
- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`

- [ ] **Step 1: Write failing sample-statistic tests**

```javascript
test("summarizeRuns reports sample SD and CV", () => {
  const summary = summarizeRuns([
    successfulRun({ wall_ms: 100, input_tokens: 800, cached_input_tokens: 600, output_tokens: 20 }),
    successfulRun({ wall_ms: 200, input_tokens: 1000, cached_input_tokens: 700, output_tokens: 30 }),
    successfulRun({ wall_ms: 300, input_tokens: 1200, cached_input_tokens: 800, output_tokens: 40 }),
  ])[0];
  assert.equal(summary.successful_wall_ms_mean, 200);
  assert.equal(summary.successful_wall_ms_sample_sd, 100);
  assert.equal(summary.successful_wall_ms_cv, 0.5);
  assert.equal(summary.successful_uncached_input_tokens_p50, 300);
  assert.equal(summary.successful_uncached_plus_output_tokens_p50, 330);
});
```

- [ ] **Step 2: Write failing 3+3 report audit test**

```javascript
test("audits a randomized three by three report", () => {
  const report = fixtureReport({
    candidateRuns: 3,
    vanillaRuns: 3,
  });
  const audit = auditBenchmarkReport(report, {
    expectedCandidateRuns: 3,
    expectedVanillaRuns: 3,
    dirtyPathPolicy: compileDirtyPathPolicy({ prefixes: ["spring-context/"] }),
    artifactExists: () => true,
    candidateEngine: "goldeneye-ack",
    markdown: renderMarkdownReport(report, {
      candidateEngine: "goldeneye-ack",
      vanillaEngine: "vanilla",
    }),
    readArtifact: () => " M spring-context/src/main/java/A.java\n",
    vanillaEngine: "vanilla",
  });
  assert.equal(audit.passed, true);
  assert.equal(audit.run_count, 6);
  assert.equal(audit.vanilla_count, 3);
});
```

- [ ] **Step 3: Verify both tests fail**

Run:

```powershell
node --test tools/agent-bench/core.test.mjs tools/agent-bench/report.test.mjs
```

Expected: FAIL because sample fields and dynamic expected counts do not exist.

- [ ] **Step 4: Add statistics helpers and derived token metrics**

```javascript
export function sampleStandardDeviation(values) {
  if (values.length < 2) return null;
  const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
  const variance = values.reduce((sum, value) => sum + ((value - mean) ** 2), 0) /
    (values.length - 1);
  return Math.sqrt(variance);
}

export function coefficientOfVariation(values) {
  if (values.length < 2) return null;
  const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
  if (mean === 0) return null;
  return sampleStandardDeviation(values) / mean;
}
```

Before grouping, derive:

```javascript
const enriched = runs.map((run) => ({
  ...run,
  uncached_input_tokens: run.input_tokens - run.cached_input_tokens,
  uncached_plus_output_tokens:
    run.input_tokens - run.cached_input_tokens + run.output_tokens,
}));
```

For `wall_ms`, `verified_e2e_ms`, `input_tokens`, `cached_input_tokens`,
`uncached_input_tokens`, `output_tokens`, `uncached_plus_output_tokens`, and
`total_tokens`, persist mean, median, range, sample SD, and CV.

- [ ] **Step 5: Generate lane-count-aware report text and audit**

Replace the fixed limitation with:

```javascript
export function renderLimitations({ candidateCount, vanillaCount, randomized }) {
  return `This benchmark contains ${candidateCount} candidate and ${vanillaCount} vanilla ` +
    `${randomized ? "randomized serial" : "serial"} runs. Results are descriptive; ` +
    "the sample is too small for inferential significance. Provider prefix caching is " +
    "reported separately from ACK snapshot caching.";
}
```

Change audit defaults without breaking the former protocol:

```javascript
const expectedCandidateRuns = options.expectedCandidateRuns ?? 3;
const expectedVanillaRuns = options.expectedVanillaRuns ?? 1;
requireAudit(candidate.length === expectedCandidateRuns,
  `expected ${expectedCandidateRuns} candidate runs, got ${candidate.length}`);
requireAudit(vanilla.length === expectedVanillaRuns,
  `expected ${expectedVanillaRuns} vanilla runs, got ${vanilla.length}`);
```

Use `evaluateDirtyPaths(...)` for status-file audit.

- [ ] **Step 6: Wire config counts into runner audit**

```javascript
expectedCandidateRuns: config.audit?.expected_candidate_runs ?? 3,
expectedVanillaRuns: config.audit?.expected_vanilla_runs ?? 1,
dirtyPathPolicy: compileDirtyPathPolicy(
  config.allowed_dirty_policy ?? { exact: config.allowed_dirty_paths ?? [] },
),
```

- [ ] **Step 7: Run tests**

Run:

```powershell
node --test tools/agent-bench/core.test.mjs tools/agent-bench/report.test.mjs
node --test tools/agent-bench/*.test.mjs
```

Expected: all tests PASS, including prior 3+1 fixtures.

- [ ] **Step 8: Commit**

```powershell
git add -- tools/agent-bench/core.mjs tools/agent-bench/core.test.mjs tools/agent-bench/report.mjs tools/agent-bench/report.test.mjs tools/agent-bench/bin/benchmark-agent-tasks.mjs
git commit -m "bench: generalize scored report audit"
```

## Task 3: Add non-scored calibration and token gates

<TASK-ID>SSRB-3</TASK-ID>

**Files:**
- Create: `tools/agent-bench/qualification.mjs`
- Create: `tools/agent-bench/qualification.test.mjs`
- Modify: `tools/agent-bench/bin/benchmark-agent-tasks.mjs`
- Modify: `tools/agent-bench/core.test.mjs`

- [ ] **Step 1: Write failing qualification tests**

```javascript
import test from "node:test";
import assert from "node:assert/strict";
import {
  evaluateCalibrationRun,
  evaluateScoredVanillaLane,
} from "./qualification.mjs";

const gates = {
  min_input_tokens: 800_000,
  max_input_tokens: 1_200_000,
  min_uncached_input_tokens: 100_000,
};

test("qualifies a passing calibration at inclusive boundaries", () => {
  const result = evaluateCalibrationRun({
    success: true,
    timed_out: false,
    grader_exit_code: 0,
    protocol_violations: [],
    input_tokens: 800_000,
    cached_input_tokens: 700_000,
  }, gates);
  assert.deepEqual(result.reasons, []);
  assert.equal(result.qualified, true);
});

test("reports every failed calibration gate", () => {
  const result = evaluateCalibrationRun({
    success: false,
    timed_out: true,
    grader_exit_code: 1,
    protocol_violations: [{ kind: "dirty-path-policy" }],
    input_tokens: 700_000,
    cached_input_tokens: 650_000,
  }, gates);
  assert.deepEqual(result.reasons, [
    "run-unsuccessful",
    "grader-failed",
    "timed-out",
    "protocol-violation",
    "input-below-minimum",
    "uncached-input-below-minimum",
  ]);
});

test("requires the scored vanilla median to remain qualified", () => {
  const result = evaluateScoredVanillaLane([
    { input_tokens: 850_000, cached_input_tokens: 700_000, success: true },
    { input_tokens: 1_000_000, cached_input_tokens: 850_000, success: true },
    { input_tokens: 1_150_000, cached_input_tokens: 990_000, success: true },
  ], gates);
  assert.equal(result.input_tokens_median, 1_000_000);
  assert.equal(result.uncached_input_tokens_median, 150_000);
  assert.equal(result.qualified, true);
});
```

- [ ] **Step 2: Run tests and verify missing-module failure**

Run:

```powershell
node --test tools/agent-bench/qualification.test.mjs
```

Expected: FAIL with `ERR_MODULE_NOT_FOUND`.

- [ ] **Step 3: Implement qualification functions**

```javascript
import { median } from "./core.mjs";

export function evaluateCalibrationRun(run, gates) {
  const uncached = run.input_tokens - run.cached_input_tokens;
  const reasons = [];
  if (!run.success) reasons.push("run-unsuccessful");
  if (run.grader_exit_code !== 0) reasons.push("grader-failed");
  if (run.timed_out) reasons.push("timed-out");
  if ((run.protocol_violations ?? []).length > 0) reasons.push("protocol-violation");
  if (run.input_tokens < gates.min_input_tokens) reasons.push("input-below-minimum");
  if (run.input_tokens > gates.max_input_tokens) reasons.push("input-above-maximum");
  if (uncached < gates.min_uncached_input_tokens) {
    reasons.push("uncached-input-below-minimum");
  }
  return {
    qualified: reasons.length === 0,
    reasons,
    input_tokens: run.input_tokens,
    cached_input_tokens: run.cached_input_tokens,
    uncached_input_tokens: uncached,
  };
}

export function evaluateScoredVanillaLane(runs, gates) {
  const inputMedian = median(runs.map((run) => run.input_tokens));
  const uncachedMedian = median(runs.map(
    (run) => run.input_tokens - run.cached_input_tokens,
  ));
  const qualified = runs.length === 3 && runs.every((run) => run.success) &&
    inputMedian >= gates.min_input_tokens &&
    inputMedian <= gates.max_input_tokens &&
    uncachedMedian >= gates.min_uncached_input_tokens;
  return {
    qualified,
    input_tokens_median: inputMedian,
    uncached_input_tokens_median: uncachedMedian,
  };
}
```

Export `median` from `core.mjs`.

- [ ] **Step 4: Add `--calibration` runner mode**

CLI contract:

```text
--calibration             run one non-scored vanilla calibration
--calibration-id <id>     immutable artifact attempt identifier
```

Hard validation:

```javascript
if (flags.has("--calibration")) {
  if (runEngines.length !== 1 || runEngines[0].kind !== "vanilla") {
    fail("--calibration requires --engine <vanilla-id>");
  }
  if (config.repetitions !== 1) fail("--calibration requires --repetitions 1");
  if (!flags.get("--calibration-id")) fail("--calibration-id is required");
}
```

Write:

```text
<run-root>/calibration/<calibration-id>/run/**
<run-root>/calibration/<calibration-id>/calibration.json
```

Never merge calibration into `report.runs`.

- [ ] **Step 5: Persist qualification evidence**

```javascript
const qualification = evaluateCalibrationRun(result, config.qualification);
persistReport(calibrationPath, {
  schema_version: 1,
  kind: "vanilla-calibration",
  calibration_id: flags.get("--calibration-id"),
  task_id: result.task_id,
  candidate: expectedCandidate,
  task_hash: result.hashes.task,
  grader_hash: result.hashes.grader,
  run: result,
  qualification,
});
```

- [ ] **Step 6: Run focused and full tests**

Run:

```powershell
node --test tools/agent-bench/qualification.test.mjs
node --test tools/agent-bench/*.test.mjs
```

Expected: all tests PASS.

- [ ] **Step 7: Commit**

```powershell
git add -- tools/agent-bench/qualification.mjs tools/agent-bench/qualification.test.mjs tools/agent-bench/core.mjs tools/agent-bench/core.test.mjs tools/agent-bench/bin/benchmark-agent-tasks.mjs
git commit -m "bench: add vanilla token qualification"
```

## Task 4: Add Level-2 Spring task and held-out grader

<TASK-ID>SSRB-4</TASK-ID>

**Files:**
- Create: `tools/agent-bench/tasks/spring-sensitive-value-redaction-level2.md`
- Create: `tools/agent-bench/graders/spring-sensitive-value-redaction.ps1`
- Create: `tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-core/SensitiveAnnotationAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-context/SensitiveDataBinderAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-context/SensitiveMethodValidationAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-web/SensitiveWebBindingInitializerAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-webmvc/SensitiveMvcAgentBenchTests.java`
- Create:
  `tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction/spring-webflux/SensitiveWebFluxAgentBenchTests.java`

- [ ] **Step 1: Write grader contract tests**

The PowerShell test must create a temporary git repository shaped like the six
Spring modules, install fake `gradlew.bat`, execute the grader, and assert:

```powershell
It "copies every held-out fixture and removes it after PASS" {
	$result = Invoke-GraderFixture -GradleExitCode 0
	$result.ExitCode | Should -Be 0
	$result.RemainingAgentBenchFiles | Should -Be @()
	$result.GradleArguments | Should -Contain ":spring-context:test"
	$result.GradleArguments | Should -Contain ":spring-webmvc:test"
	$result.GradleArguments | Should -Contain ":spring-webflux:test"
}

It "fails before Gradle when a candidate path is outside policy" {
	$result = Invoke-GraderFixture -ExtraDirtyPath "settings.gradle"
	$result.ExitCode | Should -Not -Be 0
	$result.Output | Should -Match "Protocol violation"
}

It "removes held-out fixtures after Gradle failure" {
	$result = Invoke-GraderFixture -GradleExitCode 1
	$result.ExitCode | Should -Not -Be 0
	$result.RemainingAgentBenchFiles | Should -Be @()
}
```

- [ ] **Step 2: Run grader tests and verify failure**

Run:

```powershell
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
```

Expected: FAIL because grader and fixtures do not exist.

- [ ] **Step 3: Write the agent-visible Level-2 prompt**

The task must state exact behavior from the design spec, including:

```markdown
Implement opt-in sensitive-value redaction across Spring binding and method
validation.

Required public contracts:
- runtime, documented, meta-annotatable `org.springframework.core.annotation.Sensitive`;
- pluggable detector and redactor contracts in Spring validation;
- `DataBinder` detector/redactor configuration;
- `ConfigurableWebBindingInitializer` detector/redactor configuration.

Default behavior:
- annotation-based detection;
- replacement `[REDACTED]`;
- unmarked values unchanged.

Cover bean/direct-field access, conversion failures, validator rejection,
constructor binding, nested/indexed paths, method arguments, MVC, and WebFlux.
Redact error representations only. Never mutate target values or invocation
arguments. Preserve message codes, error arguments, binding-failure flags,
container indexes/keys, and source unwrapping.

Add focused production tests and run relevant module tests. Do not run `clean`.
Do not change build scripts, dependency declarations, generated files, or files
outside spring-core, spring-beans, spring-context, spring-web, spring-webmvc,
and spring-webflux.
```

Do not list expected implementation files.

- [ ] **Step 4: Add hidden annotation and DataBinder fixtures**

`SensitiveAnnotationAgentBenchTests` must assert runtime retention, field,
method, parameter, record-component, annotation-type targets, and composed
annotation discovery.

`SensitiveDataBinderAgentBenchTests` must include:

```java
record Credentials(String username, @Sensitive String password) {
}

@Test
void redactsValidatorRejectedRecordComponentWithoutMutatingTarget() {
	Credentials target = new Credentials("spring", "s3cr3t");
	DataBinder binder = new DataBinder(target, "credentials");
	binder.addValidators((object, errors) ->
			errors.rejectValue("password", "weak"));
	binder.validate();

	FieldError error = binder.getBindingResult().getFieldError("password");
	assertThat(error).isNotNull();
	assertThat(error.getRejectedValue()).isEqualTo("[REDACTED]");
	assertThat(target.password()).isEqualTo("s3cr3t");
	assertThat(error.getCode()).isEqualTo("weak");
}
```

Additional complete test methods must assert:

- unmarked rejected value remains original;
- type mismatch redacts submitted secret;
- direct field access redacts;
- nested `accounts[0].password` redacts;
- custom detector marks an unannotated property;
- custom redactor returns `"<hidden:credentials.password>"`;
- `FieldError.isBindingFailure()`, codes, arguments, and source unwrap survive.

- [ ] **Step 5: Add method-validation fixture**

Create a service method with `@Sensitive @Size(min = 12) String token`, validate
through `MethodValidationAdapter`, and assert:

```java
ParameterValidationResult result =
		validationResult.getParameterValidationResults().get(0);
assertThat(result.getArgument()).isEqualTo("[REDACTED]");
assertThat(originalArguments[0]).isEqualTo("short");
assertThat(result.getResolvableErrors()).hasSize(1);
```

Also assert an unmarked parameter remains unchanged and source unwrapping still
returns the underlying `ConstraintViolation`.

- [ ] **Step 6: Add web initializer, MVC, and WebFlux fixtures**

The initializer test must configure a custom redactor, invoke
`initBinder(WebDataBinder)`, trigger a rejection, and observe the custom marker.

MVC and WebFlux tests must use their existing test infrastructure to submit a
secret to annotated model/request objects, obtain the resulting
`BindingResult` or validation exception, and assert:

```java
assertThat(fieldError.getRejectedValue()).isEqualTo("[REDACTED]");
assertThat(fieldError.toString()).doesNotContain("s3cr3t");
assertThat(exception.toString()).doesNotContain("s3cr3t");
```

Each test also asserts the controller target or captured invocation argument
still contains `s3cr3t`.

- [ ] **Step 7: Implement grader fixture installation and cleanup**

Use a manifest:

```powershell
$Fixtures = @(
	@{ Module = "spring-core"; Source = "spring-core/SensitiveAnnotationAgentBenchTests.java";
		Target = "spring-core/src/test/java/org/springframework/core/annotation/SensitiveAnnotationAgentBenchTests.java";
		Test = "org.springframework.core.annotation.SensitiveAnnotationAgentBenchTests" },
	@{ Module = "spring-context"; Source = "spring-context/SensitiveDataBinderAgentBenchTests.java";
		Target = "spring-context/src/test/java/org/springframework/validation/SensitiveDataBinderAgentBenchTests.java";
		Test = "org.springframework.validation.SensitiveDataBinderAgentBenchTests" },
	@{ Module = "spring-context"; Source = "spring-context/SensitiveMethodValidationAgentBenchTests.java";
		Target = "spring-context/src/test/java/org/springframework/validation/beanvalidation/SensitiveMethodValidationAgentBenchTests.java";
		Test = "org.springframework.validation.beanvalidation.SensitiveMethodValidationAgentBenchTests" },
	@{ Module = "spring-web"; Source = "spring-web/SensitiveWebBindingInitializerAgentBenchTests.java";
		Target = "spring-web/src/test/java/org/springframework/web/bind/support/SensitiveWebBindingInitializerAgentBenchTests.java";
		Test = "org.springframework.web.bind.support.SensitiveWebBindingInitializerAgentBenchTests" },
	@{ Module = "spring-webmvc"; Source = "spring-webmvc/SensitiveMvcAgentBenchTests.java";
		Target = "spring-webmvc/src/test/java/org/springframework/web/servlet/mvc/method/annotation/SensitiveMvcAgentBenchTests.java";
		Test = "org.springframework.web.servlet.mvc.method.annotation.SensitiveMvcAgentBenchTests" },
	@{ Module = "spring-webflux"; Source = "spring-webflux/SensitiveWebFluxAgentBenchTests.java";
		Target = "spring-webflux/src/test/java/org/springframework/web/reactive/result/method/annotation/SensitiveWebFluxAgentBenchTests.java";
		Test = "org.springframework.web.reactive.result.method.annotation.SensitiveWebFluxAgentBenchTests" }
)
```

Copy with `Copy-Item -LiteralPath`. In `finally`, remove only manifest targets.
Run each module once with all its selectors and `--build-cache`. Fail on first
non-zero Gradle exit. Before and after fixture installation, enforce candidate
dirty paths through the same module policy as config.

- [ ] **Step 8: Run grader contract tests**

Run:

```powershell
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
```

Expected: PASS.

- [ ] **Step 9: Commit**

```powershell
git add -- tools/agent-bench/tasks/spring-sensitive-value-redaction-level2.md tools/agent-bench/graders/spring-sensitive-value-redaction.ps1 tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1 tools/agent-bench/graders/fixtures/spring-sensitive-value-redaction
git commit -m "bench: add Spring sensitive redaction task"
```

## Task 5: Add Level-2 configuration and six-module snapshot

<TASK-ID>SSRB-5</TASK-ID>

**Files:**
- Create:
  `tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json`
- Test: `tools/agent-bench/core.test.mjs`
- Generate:
  `target/agent-bench/snapshots/spring-sensitive-value-redaction-level2/**`

- [ ] **Step 1: Add a failing config dry-run test**

Add a fixture config test that loads the new JSON and asserts:

```javascript
assert.deepEqual(config.qualification, {
  min_input_tokens: 800_000,
  max_input_tokens: 1_200_000,
  min_uncached_input_tokens: 100_000,
});
assert.deepEqual(config.audit, {
  expected_candidate_runs: 3,
  expected_vanilla_runs: 3,
});
assert.equal(config.allowed_dirty_policy.max_paths, 40);
```

- [ ] **Step 2: Run test and verify missing-config failure**

Run:

```powershell
node --test tools/agent-bench/core.test.mjs
```

Expected: FAIL with missing Level-2 config.

- [ ] **Step 3: Create pinned config**

Key values:

```json
{
  "name": "spring-sensitive-value-redaction-level2",
  "output": "../../../target/agent-bench/spring-sensitive-value-redaction-level2/report.json",
  "repo": "D:\\Dev\\IdeaProjects\\spring-framework",
  "base_ref": "daf955157871e4ac6f192e06b71d6cc595eb979b",
  "model": "gpt-5.6-terra",
  "reasoning": "high",
  "codex_full_access": true,
  "repetitions": 3,
  "timeout_ms": 3600000,
  "cache_modes": ["warm"],
  "qualification": {
    "min_input_tokens": 800000,
    "max_input_tokens": 1200000,
    "min_uncached_input_tokens": 100000
  },
  "audit": {
    "expected_candidate_runs": 3,
    "expected_vanilla_runs": 3
  },
  "allowed_dirty_policy": {
    "prefixes": [
      "spring-core/src/",
      "spring-beans/src/",
      "spring-context/src/",
      "spring-web/src/",
      "spring-webmvc/src/",
      "spring-webflux/src/"
    ],
    "required_prefixes": [
      "spring-core/src/main/java/",
      "spring-context/src/main/java/",
      "spring-web/src/main/java/"
    ],
    "min_paths": 8,
    "max_paths": 40
  }
}
```

Configure the task, grader, ACK engine, vanilla engine, Java home, Gradle home,
stable worktree, live cache, and snapshot roots exactly as the prior Spring
config, using new unique paths.

Set `GOLDENEYE_INCLUDE_PATHS` to the `src/main/java` and `src/test/java` trees
of all six modules.

- [ ] **Step 4: Verify dry-run matrix**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --repetitions 3 `
  --seed 20260725 `
  --dry-run
```

Expected: six unique runs, three `goldeneye-ack/warm` and three
`vanilla/none`.

- [ ] **Step 5: Prepare immutable snapshot**

Run:

```powershell
$env:JAVA_HOME='C:\Users\Zacha\.jdks\openjdk-17.0.2'
$env:GRADLE_USER_HOME='D:\Dev\Caches\gradle-spring-framework-6.2'
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --engine goldeneye-ack `
  --prepare-snapshot
```

Expected: eligible preparation, manifest SHA-256, no writer artifacts, no
Codex process.

- [ ] **Step 6: Verify preparation**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --verify-only
```

Expected: `Preparation gates: ELIGIBLE`.

- [ ] **Step 7: Run harness tests**

Run:

```powershell
node --test tools/agent-bench/*.test.mjs
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
```

Expected: all tests PASS.

- [ ] **Step 8: Commit config**

Do not commit snapshot artifacts.

```powershell
git add -- tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json tools/agent-bench/core.test.mjs
git commit -m "bench: configure million token Spring task"
```

## Task 6: Freeze benchmark provenance

<TASK-ID>SSRB-6</TASK-ID>

**Files:**
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/provenance.json`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/preparation.json`

- [ ] **Step 1: Run full harness verification**

Run:

```powershell
node --test tools/agent-bench/*.test.mjs
pwsh -NoProfile -File tools/agent-bench/graders/spring-sensitive-value-redaction.test.ps1
git diff --check
```

Expected: all tests PASS; whitespace check empty.

- [ ] **Step 2: Build and verify frozen candidate**

Run:

```powershell
cargo test -p goldeneye-ack
cargo build --release -p goldeneye
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --verify-only
```

Expected: Rust tests PASS, release build exits `0`, preparation eligible.

- [ ] **Step 3: Verify source invariants**

Run:

```powershell
git status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' rev-parse HEAD
```

Expected: both statuses empty; Spring HEAD
`daf955157871e4ac6f192e06b71d6cc595eb979b`.

- [ ] **Step 4: Record frozen hashes**

The preparation/provenance artifacts must include:

```json
{
  "candidate_commit": "value produced by git rev-parse HEAD",
  "candidate_executable_sha256": "value produced by provenance.mjs",
  "ack_bundle_sha256": "value produced by provenance.mjs",
  "task_sha256": "value produced by provenance.mjs",
  "grader_sha256": "value produced by provenance.mjs over grader and fixture manifest",
  "config_sha256": "value produced by provenance.mjs",
  "snapshot_manifest_sha256": "value produced by snapshot.mjs",
  "spring_commit": "daf955157871e4ac6f192e06b71d6cc595eb979b"
}
```

- [ ] **Step 5: Commit any provenance-code fix only after re-running Steps 1–4**

Expected: no uncommitted source change before calibration.

## Task 7: Calibrate vanilla to the one-million-token gate

<TASK-ID>SSRB-7</TASK-ID>

**Files:**
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/calibration/level2-attempt1/**`
- Create on the below-floor branch of the predeclared ladder:
  `tools/agent-bench/tasks/spring-sensitive-value-redaction-level3.md`
- Create on the below-floor branch of the predeclared ladder:
  `tools/agent-bench/configs/spring-sensitive-value-redaction-level3.json`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/qualification-selection.json`

- [ ] **Step 1: Execute exactly one Level-2 clean vanilla calibration**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json `
  --engine vanilla `
  --repetitions 1 `
  --calibration `
  --calibration-id level2-attempt1
```

Expected: versioned calibration artifact, grader result, qualification object;
no scored report run.

- [ ] **Step 2: Apply the predeclared decision table**

```text
PASS + 800k–1.2M input + ≥100k uncached  -> freeze Level 2
PASS + below either floor                -> implement Level 3 exactly as spec
PASS + above 1.2M input                  -> implement Level 1 exactly as spec
grader/harness defect                    -> preserve attempt, fix defect, new versioned attempt
genuine agent correctness failure        -> preserve attempt; do not count or silently rerun
```

- [ ] **Step 3: If required, add Level-3 prompt and fixtures using TDD**

Level 3 adds only:

- composed marker annotations;
- record/Kotlin-compatible accessor metadata;
- nested container-element paths;
- custom detector composition;
- context-aware custom redaction;
- formatting, equality, hashing, serialization, and source-unwrapping leak
  checks.

Run grader contract tests before and after implementation, commit the versioned
task/config, prepare a new snapshot only if include paths change, and use
calibration ID `level3-attempt1`.

- [ ] **Step 4: Re-run verification after the qualifying calibration**

Run:

```powershell
$QualifiedConfig = 'tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json'
# On the Level-3 decision branch, assign:
# $QualifiedConfig = 'tools/agent-bench/configs/spring-sensitive-value-redaction-level3.json'
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config $QualifiedConfig `
  --verify-only
git status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
```

Expected: eligible preparation and clean repositories.

- [ ] **Step 5: Record qualification**

Persist chosen level, attempt ID, exact tokens, grader exit, task/grader hashes,
and qualification reasons. Calibration remains excluded from scored summary.
Write the selected config and scored report paths:

```json
{
  "level": 2,
  "calibration_id": "level2-attempt1",
  "config": "tools/agent-bench/configs/spring-sensitive-value-redaction-level2.json",
  "provenance": "target/agent-bench/spring-sensitive-value-redaction-level2/provenance.json",
  "scored_report": "target/agent-bench/spring-sensitive-value-redaction-level2/report.json",
  "qualified": true
}
```

On the Level-3 branch, write the same fields with level `3`, calibration ID
`level3-attempt1`, Level-3 config, provenance, and report paths.

## Task 8: Execute and audit the randomized clean 3×3 benchmark

<TASK-ID>SSRB-8</TASK-ID>

Execute exactly three vanilla and three warm ACK runs after qualification.

**Files:**
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/scored/**`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/report.json`
- Generate:
  `target/agent-bench/spring-sensitive-value-redaction-level2/report.md`

- [ ] **Step 1: Derive and record seed before execution**

Use SHA-256 of:

```text
spring-sensitive-value-redaction|<task_sha256>|<candidate_executable_sha256>
```

Interpret the first eight hex digits as an unsigned integer. Store seed in
an execution manifest before dry-run:

```powershell
$SelectionPath = 'target/agent-bench/spring-sensitive-value-redaction-level2/qualification-selection.json'
$Selection = Get-Content -Raw -LiteralPath $SelectionPath | ConvertFrom-Json
if (-not $Selection.qualified) { throw "Qualification selection is not qualified" }
$QualifiedConfig = $Selection.config
$ScoredReport = $Selection.scored_report
$Provenance = Get-Content -Raw -LiteralPath $Selection.provenance | ConvertFrom-Json
$SeedMaterial = "spring-sensitive-value-redaction|$($Provenance.task_sha256)|$($Provenance.candidate_executable_sha256)"
$SeedHash = [Convert]::ToHexString(
	[Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes($SeedMaterial))
).ToLowerInvariant()
$FrozenSeed = [Convert]::ToUInt32($SeedHash.Substring(0, 8), 16)
@{
	selection = $SelectionPath
	config = $QualifiedConfig
	scored_report = $ScoredReport
	seed_material = $SeedMaterial
	seed_sha256 = $SeedHash
	seed = $FrozenSeed
} | ConvertTo-Json | Set-Content -LiteralPath 'target/agent-bench/spring-sensitive-value-redaction-level2/execution-manifest.json'
```

- [ ] **Step 2: Dry-run the frozen six-run matrix**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config $QualifiedConfig `
  --repetitions 3 `
  --seed $FrozenSeed `
  --out $ScoredReport `
  --dry-run
```

Expected: exactly six unique randomized IDs, three per lane.

- [ ] **Step 3: Execute all six runs serially in one invocation**

Run the same command without `--dry-run`.

Expected: six raw artifact directories. Any hard provenance/snapshot/source
gate failure aborts remaining runs.

- [ ] **Step 4: Audit the report**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs `
  --config $QualifiedConfig `
  --out $ScoredReport `
  --audit-report
```

Expected:

```text
Audit: PASS runs=6 candidate=3 vanilla=3 violations=0
```

- [ ] **Step 5: Verify scored vanilla median qualification**

Read the persisted `report.summary` entry for `vanilla/none` and require:

```text
successes = 3
800,000 ≤ successful_input_tokens_p50 ≤ 1,200,000
successful_uncached_input_tokens_p50 ≥ 100,000
```

If the scored median misses a token gate, preserve the complete report as a
non-qualifying scored attempt. Return to the predeclared ladder; do not alter or
discard the report.

- [ ] **Step 6: Verify final invariants**

Run:

```powershell
node tools/agent-bench/bin/benchmark-agent-tasks.mjs --config $QualifiedConfig --verify-only
git status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' status --short
git -C 'D:\Dev\IdeaProjects\spring-framework' rev-parse HEAD
```

Also verify no `node.exe`, `codex.exe`, grader, or temporary scored worktree
process/path remains.

Expected: preparation eligible, both repositories clean, pinned Spring commit,
zero matching processes, immutable snapshot hash unchanged.

- [ ] **Step 7: Publish final analysis**

Report:

- every raw value;
- median, range, sample SD, and CV%;
- correctness;
- cached and uncached tokens separately;
- ACK call/failure/discovery counts;
- patch statistics;
- qualification evidence;
- built-in and independent audit evidence;
- limitations at `n = 3`.

State whether ACK improved or regressed each metric. Do not claim statistical
significance.

- [ ] **Step 8: Commit final benchmark documentation only**

Do not commit raw target artifacts unless repository policy explicitly requires
it.

```powershell
git add -- docs/superfastpowers/plans/SSRB
git commit -m "docs: report million token Spring benchmark"
```
