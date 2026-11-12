import { describe, expect, it } from "vitest";
import {
  largeMethodSnippetResponse,
  methodSnippetResponse,
} from "./fixtures/codebaseMemory.js";
import { normalizeSelectedSymbol } from "../src/adapters/codebaseMemoryMcp/normalize.js";
import { contextAffordanceWarnings } from "../src/kernel/affordanceSignals.js";
import {
  DEFAULT_CALLER_TRACE_THRESHOLD,
  callerTraceThresholdFromEnv,
} from "../src/kernel/inspectPolicy.js";
import { inboundTraceDecision } from "../src/kernel/tracePolicy.js";

describe("context-safety policies", () => {
  it("does not warn for compact explicit methods", () => {
    expect(contextAffordanceWarnings(normalizeSelectedSymbol(methodSnippetResponse))).toEqual([]);
  });

  it("warns when large methods reduce context affordance", () => {
    expect(contextAffordanceWarnings(normalizeSelectedSymbol(largeMethodSnippetResponse))).toContain(
      "large method; source likely needed",
    );
  });

  it("warns when complexity is high", () => {
    const selected = {
      ...normalizeSelectedSymbol(methodSnippetResponse),
      complexity: 10,
      cognitive: 15,
    };

    expect(contextAffordanceWarnings(selected)).toContain(
      "high complexity; inspect related callers and tests before editing",
    );
  });

  it("warns when caller count is high", () => {
    const selected = {
      ...normalizeSelectedSymbol(methodSnippetResponse),
      callers: 9,
    };

    expect(contextAffordanceWarnings(selected)).toContain(
      "high caller count; use callers command rather than inline trace",
    );
  });

  it("returns a trace hint when caller count exceeds threshold", () => {
    expect(
      inboundTraceDecision({
        qualifiedName: "com.example.booking.BookingService.reconcileBooking",
        callerCount: 12,
        threshold: 5,
      }),
    ).toEqual({
      kind: "hint",
      count: 12,
      command: "gcal callers com.example.booking.BookingService.reconcileBooking --depth 1",
    });
  });

  it("does not return a trace hint when caller count is at threshold", () => {
    expect(
      inboundTraceDecision({
        qualifiedName: "com.example.booking.BookingService.cancelBooking",
        callerCount: DEFAULT_CALLER_TRACE_THRESHOLD,
        threshold: DEFAULT_CALLER_TRACE_THRESHOLD,
      }),
    ).toBeNull();
  });

  it("defaults caller trace threshold for missing or invalid environment values", () => {
    expect(callerTraceThresholdFromEnv({})).toBe(DEFAULT_CALLER_TRACE_THRESHOLD);
    expect(callerTraceThresholdFromEnv({ GCAL_CALLER_TRACE_THRESHOLD: "" })).toBe(
      DEFAULT_CALLER_TRACE_THRESHOLD,
    );
    expect(callerTraceThresholdFromEnv({ GCAL_CALLER_TRACE_THRESHOLD: "abc" })).toBe(
      DEFAULT_CALLER_TRACE_THRESHOLD,
    );
    expect(callerTraceThresholdFromEnv({ GCAL_CALLER_TRACE_THRESHOLD: "-1" })).toBe(
      DEFAULT_CALLER_TRACE_THRESHOLD,
    );
  });

  it("parses valid caller trace threshold environment values", () => {
    expect(callerTraceThresholdFromEnv({ GCAL_CALLER_TRACE_THRESHOLD: "8" })).toBe(8);
  });
});
