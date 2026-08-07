import { performance } from "node:perf_hooks";
import { finished } from "node:stream/promises";

export async function closeWritableStream(stream) {
  if (stream.closed || stream.writableFinished) return;
  const completion = finished(stream);
  if (!stream.writableEnded) stream.end();
  await completion;
}

export function isAgentProtocolCompletionLine(line) {
  try {
    return JSON.parse(line).type === "turn.completed";
  } catch {
    return false;
  }
}

export function createAgentCompletionController({
  child,
  timeoutMs,
  completionGraceMs = 5_000,
  elapsedMs,
  terminate,
}) {
  let settled = false;
  let timedOut = false;
  let protocolCompleted = false;
  let forcedAfterCompletion = false;
  let durationAtProtocolCompletion = null;
  let completionTimer = null;
  let settleOutcome;

  const timeoutTimer = setTimeout(() => {
    timedOut = true;
    terminate(child.pid);
  }, timeoutMs);

  const outcome = new Promise((resolveOutcome) => {
    const settle = ({ exitCode, error }) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeoutTimer);
      clearTimeout(completionTimer);
      resolveOutcome({
        durationMs: durationAtProtocolCompletion ?? elapsedMs(),
        exitCode: forcedAfterCompletion ? 0 : exitCode,
        error,
        timedOut,
        protocolCompleted,
        forcedAfterCompletion,
      });
    };
    settleOutcome = settle;
    child.once("error", (error) => settle({ exitCode: null, error }));
    child.once("exit", (exitCode) => {
      if (protocolCompleted) settle({ exitCode, error: null });
    });
    child.once("close", (exitCode) => settle({ exitCode, error: null }));
  });

  return {
    outcome,
    markProtocolCompleted() {
      if (settled || protocolCompleted) return;
      protocolCompleted = true;
      durationAtProtocolCompletion = elapsedMs();
      clearTimeout(timeoutTimer);
      if (child.exitCode !== null || child.signalCode !== null) {
        settleOutcome({ exitCode: child.exitCode, error: null });
        return;
      }
      completionTimer = setTimeout(() => {
        if (settled) return;
        forcedAfterCompletion = true;
        terminate(child.pid);
      }, completionGraceMs);
    },
  };
}

export function spawnWithTimer(
  spawnFn,
  now = performance.now.bind(performance),
) {
  const startedAt = now();
  const child = spawnFn();
  return {
    child,
    elapsedMs: () => now() - startedAt,
  };
}

export function stopTimerAtClose(measured) {
  return measured.elapsedMs();
}

export function scoreRunDurations({ maintenanceMs, wallMs, graderMs }) {
  return {
    maintenance_ms: maintenanceMs,
    wall_ms: wallMs,
    grader_ms: graderMs,
    completion_ms: wallMs,
    verified_e2e_ms: wallMs + graderMs,
  };
}
