export const DEFAULT_CALLER_TRACE_THRESHOLD = 5;

export function callerTraceThresholdFromEnv(env: NodeJS.ProcessEnv): number {
  const raw = env.GCAL_CALLER_TRACE_THRESHOLD;
  if (raw === undefined || raw.trim() === "") {
    return DEFAULT_CALLER_TRACE_THRESHOLD;
  }

  const parsed = Number.parseInt(raw, 10);
  if (!Number.isFinite(parsed) || parsed < 0) {
    return DEFAULT_CALLER_TRACE_THRESHOLD;
  }

  return parsed;
}
