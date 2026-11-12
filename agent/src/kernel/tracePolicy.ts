import type { TraceHint } from "../domain/types.js";

export interface TraceDecisionInput {
  qualifiedName: string;
  callerCount: number | null;
  threshold: number;
}

export function inboundTraceDecision(input: TraceDecisionInput): TraceHint | null {
  const count = input.callerCount ?? 0;
  if (count <= input.threshold) {
    return null;
  }

  return {
    kind: "hint",
    count,
    command: `gcal callers ${input.qualifiedName} --depth 1`,
  };
}
