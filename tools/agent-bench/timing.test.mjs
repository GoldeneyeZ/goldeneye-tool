import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import test from "node:test";

import {
  closeWritableStream,
  createAgentCompletionController,
  isAgentProtocolCompletionLine,
  scoreRunDurations,
  spawnWithTimer,
  stopTimerAtClose,
} from "./timing.mjs";

test("closeWritableStream resolves after an already-ended stream", async () => {
  const stream = new PassThrough();
  stream.resume();
  stream.end("complete");
  await new Promise((resolve) => stream.once("finish", resolve));

  await closeWritableStream(stream);
  assert.equal(stream.writableFinished, true);
});

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

test("close-boundary duration excludes post-close artifact flushing", () => {
  let now = 1_000;
  const measured = spawnWithTimer(() => ({ pid: 42 }), () => now);
  now += 25; // child exits
  const durationAtClose = stopTimerAtClose(measured);
  now += 400; // JSONL and stderr streams flush after child exit
  assert.equal(durationAtClose, 25);
});

test("protocol completion terminates a lingering agent without marking timeout", async () => {
  const child = new EventEmitter();
  child.pid = 42;
  child.exitCode = null;
  child.signalCode = null;
  let now = 1_000;
  let terminatedPid = null;
  const controller = createAgentCompletionController({
    child,
    timeoutMs: 1_000,
    completionGraceMs: 5,
    elapsedMs: () => now - 1_000,
    terminate: (pid) => {
      terminatedPid = pid;
      child.signalCode = "SIGKILL";
      child.emit("close", 1);
    },
  });

  now += 120;
  controller.markProtocolCompleted();
  const outcome = await controller.outcome;

  assert.equal(terminatedPid, 42);
  assert.deepEqual(outcome, {
    durationMs: 120,
    exitCode: 0,
    error: null,
    timedOut: false,
    protocolCompleted: true,
    forcedAfterCompletion: true,
  });
});

test("protocol completion settles on process exit when stdio remains open", async () => {
  const child = new EventEmitter();
  child.pid = 42;
  child.exitCode = null;
  child.signalCode = null;
  const controller = createAgentCompletionController({
    child,
    timeoutMs: 1_000,
    completionGraceMs: 1_000,
    elapsedMs: () => 120,
    terminate: () => {},
  });

  controller.markProtocolCompleted();
  child.exitCode = 0;
  child.emit("exit", 0);
  const outcome = await Promise.race([
    controller.outcome,
    new Promise((resolve) => setTimeout(() => resolve("still-waiting"), 25)),
  ]);

  assert.notEqual(outcome, "still-waiting");
  assert.deepEqual(outcome, {
    durationMs: 120,
    exitCode: 0,
    error: null,
    timedOut: false,
    protocolCompleted: true,
    forcedAfterCompletion: false,
  });
});

test("protocol completion requires a top-level turn.completed event", () => {
  assert.equal(
    isAgentProtocolCompletionLine(JSON.stringify({ type: "turn.completed", usage: {} })),
    true,
  );
  assert.equal(
    isAgentProtocolCompletionLine(
      JSON.stringify({ type: "item.completed", item: { text: "turn.completed" } }),
    ),
    false,
  );
  assert.equal(isAgentProtocolCompletionLine("not json"), false);
});
