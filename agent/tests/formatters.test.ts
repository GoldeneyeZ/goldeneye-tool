import { describe, expect, it } from "vitest";
import { methodSnippetResponse, searchGraphResponse } from "./fixtures/codebaseMemory.js";
import {
  normalizeSearchResponse,
  normalizeSelectedSymbol,
} from "../src/adapters/codebaseMemoryMcp/normalize.js";
import type { SelectedSymbol, SymbolCandidate, TraceEdge, TraceHint } from "../src/domain/types.js";
import { formatCompactJson } from "../src/formatters/jsonFormatters.js";
import {
  formatCandidateBlockText,
  formatCandidatesText,
  formatSelectedMetadataText,
  formatSourceText,
  formatTraceRowsText,
  formatTraceSectionText,
} from "../src/formatters/textFormatters.js";

describe("formatters", () => {
  it("formats search candidates as compact tab-separated rows", () => {
    const rows = normalizeSearchResponse(searchGraphResponse);

    expect(formatCandidatesText(rows)).toBe(
      "com.example.booking.BookingService.cancelBooking\tMethod\tsrc/main/java/com/example/booking/BookingService.java\t42\tpublic BookingResponse cancelBooking(String bookingId)",
    );
  });

  it("formats candidate blocks with stable row numbers", () => {
    const rows = normalizeSearchResponse(searchGraphResponse);

    expect(formatCandidateBlockText(rows)).toBe(
      [
        "# candidates",
        "1\tMethod\tcom.example.booking.BookingService.cancelBooking\tsrc/main/java/com/example/booking/BookingService.java:42\tpublic BookingResponse cancelBooking(String bookingId)",
      ].join("\n"),
    );
  });

  it("formats inspect selected metadata without source", () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);
    const output = formatSelectedMetadataText(selected, ["large_symbol"]);

    expect(output).toBe(
      [
        "# selected",
        "qualified_name=com.example.booking.BookingService.cancelBooking",
        "kind=Method",
        "file=src/main/java/com/example/booking/BookingService.java",
        "line=42",
        "end_line=58",
        "lines=17",
        "complexity=3",
        "cognitive=4",
        "visibility=",
        "signature=public BookingResponse cancelBooking(String bookingId)",
        "return_type=BookingResponse",
        "decorators=",
        "callers=4",
        "callees=2",
        "warnings=large_symbol",
      ].join("\n"),
    );
    expect(output).not.toContain("resolveActiveBooking");
    expect(output).not.toContain(selected.source);
  });

  it("formats empty metadata fields as empty values", () => {
    const selected: SelectedSymbol = {
      qualifiedName: "com.example.EmptySymbol",
      kind: "Unknown",
      filePath: "",
      startLine: null,
      endLine: null,
      lines: null,
      complexity: null,
      cognitive: null,
      visibility: "",
      signature: "",
      returnType: "",
      decorators: "",
      callers: null,
      callees: null,
      source: "source must not render",
    };

    expect(formatSelectedMetadataText(selected)).toBe(
      [
        "# selected",
        "qualified_name=com.example.EmptySymbol",
        "kind=Unknown",
        "file=",
        "line=",
        "end_line=",
        "lines=",
        "complexity=",
        "cognitive=",
        "visibility=",
        "signature=",
        "return_type=",
        "decorators=",
        "callers=",
        "callees=",
        "warnings=",
      ].join("\n"),
    );
  });

  it("formats get output as source only", () => {
    const selected = normalizeSelectedSymbol(methodSnippetResponse);

    expect(formatSourceText(selected)).toBe(selected.source);
  });

  it("formats trace sections using trace edge qualified names", () => {
    const trace: TraceEdge[] = [
      {
        sourceQualifiedName: "com.example.booking.BookingController.cancelBooking",
        targetQualifiedName: "com.example.booking.BookingService.cancelBooking",
        relatedQualifiedName: "com.example.booking.BookingController.cancelBooking",
        hop: 1,
        filePath: "src/main/java/com/example/booking/BookingController.java",
        line: 31,
      },
    ];

    expect(formatTraceSectionText("inbound", trace)).toBe(
      [
        "# inbound",
        "com.example.booking.BookingController.cancelBooking\t1\tsrc/main/java/com/example/booking/BookingController.java\t31",
      ].join("\n"),
    );
  });

  it("formats standalone trace rows without section headers", () => {
    const trace: TraceEdge[] = [
      {
        sourceQualifiedName: "com.example.booking.BookingController.cancelBooking",
        targetQualifiedName: "com.example.booking.BookingService.cancelBooking",
        relatedQualifiedName: "com.example.booking.BookingController.cancelBooking",
        hop: 1,
        filePath: "src/main/java/com/example/booking/BookingController.java",
        line: 31,
      },
    ];

    expect(formatTraceRowsText(trace)).toBe(
      "com.example.booking.BookingController.cancelBooking\t1\tsrc/main/java/com/example/booking/BookingController.java\t31",
    );
  });

  it("formats empty standalone trace rows as empty output", () => {
    expect(formatTraceRowsText([])).toBe("");
  });

  it("formats trace hints as compact hint rows", () => {
    const trace: TraceHint = {
      kind: "hint",
      count: 12,
      command: "gcal callers com.example.booking.BookingService.cancelBooking --depth 1",
    };

    expect(formatTraceSectionText("inbound", trace)).toBe(
      [
        "# inbound",
        "hint\t12\tgcal callers com.example.booking.BookingService.cancelBooking --depth 1",
      ].join("\n"),
    );
  });

  it("formats empty candidate lists as a header only", () => {
    expect(formatCandidatesText([])).toBe("");
    expect(formatCandidateBlockText([])).toBe("# candidates");
  });

  it("formats null candidate line values as empty fields", () => {
    const rows: SymbolCandidate[] = [
      {
        qualifiedName: "com.example.booking.BookingService",
        label: "Class",
        filePath: "src/main/java/com/example/booking/BookingService.java",
        line: null,
        signature: "",
      },
    ];

    expect(formatCandidatesText(rows)).toBe(
      "com.example.booking.BookingService\tClass\tsrc/main/java/com/example/booking/BookingService.java\t\t",
    );
    expect(formatCandidateBlockText(rows)).toBe(
      [
        "# candidates",
        "1\tClass\tcom.example.booking.BookingService\tsrc/main/java/com/example/booking/BookingService.java\t",
      ].join("\n"),
    );
  });

  it("formats compact JSON on one line", () => {
    expect(formatCompactJson({ project: "example-project", indexed: true })).toBe(
      '{"project":"example-project","indexed":true}',
    );
  });
});
