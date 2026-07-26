## Task 2: Generalize report audit and variability statistics

<TASK-ID>SSRB-2</TASK-ID>

**Files:**
- Modify: `tools/agent-bench/core.mjs`
- Modify: `tools/agent-bench/core.test.mjs`
- Modify: `tools/agent-bench/report.mjs`
- Modify: `tools/agent-bench/report.test.mjs`
- Modify: `tools/benchmark-agent-tasks.mjs`

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
git add -- tools/agent-bench/core.mjs tools/agent-bench/core.test.mjs tools/agent-bench/report.mjs tools/agent-bench/report.test.mjs tools/benchmark-agent-tasks.mjs
git commit -m "bench: generalize scored report audit"
```
