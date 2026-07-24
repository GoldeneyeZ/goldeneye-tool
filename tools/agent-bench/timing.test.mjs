import assert from "node:assert/strict";
import test from "node:test";

import { scoreRunDurations, spawnWithTimer } from "./timing.mjs";

test("spawn timer excludes pre-spawn maintenance and includes spawn callback work", () => {
  let now = 1_000;
  now += 400;
  const measured = spawnWithTimer(
    () => {
      now += 25;
      return { pid: 42 };
    },
    () => now,
  );
  now += 75;
  assert.equal(measured.child.pid, 42);
  assert.equal(measured.elapsedMs(), 100);
});

test("scoreRunDurations keeps maintenance outside completion", () => {
  assert.deepEqual(
    scoreRunDurations({ maintenanceMs: 500, wallMs: 100, graderMs: 20 }),
    {
      maintenance_ms: 500,
      wall_ms: 100,
      grader_ms: 20,
      completion_ms: 100,
      verified_e2e_ms: 120,
    },
  );
});
