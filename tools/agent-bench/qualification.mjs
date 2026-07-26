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
