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
