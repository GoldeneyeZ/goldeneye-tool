import { performance } from "node:perf_hooks";
import { finished } from "node:stream/promises";

export async function closeWritableStream(stream) {
  if (stream.closed || stream.writableFinished) return;
  const completion = finished(stream);
  if (!stream.writableEnded) stream.end();
  await completion;
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
