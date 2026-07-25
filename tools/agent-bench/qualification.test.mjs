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
