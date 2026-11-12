import { describe, expect, it } from "vitest";
import {
  architectureResponse,
  largeMethodSnippetResponse,
  methodSnippetResponse,
  searchGraphResponse,
} from "./fixtures/codebaseMemory.js";
import {
  normalizeArchitectureResponse,
  normalizeProjectsResponse,
  normalizeSearchResponse,
  normalizeSelectedSymbol,
} from "../src/adapters/codebaseMemoryMcp/normalize.js";

describe("codebase-memory response normalization", () => {
  it("normalizes indexed project names and roots", () => {
    expect(
      normalizeProjectsResponse({
        projects: [
          { name: "example-project", root_path: "C:/code/example" },
          { project_id: "second-project", path: "C:/code/second" },
          { name: "missing-root" },
        ],
      }),
    ).toEqual([
      { name: "example-project", rootPath: "C:/code/example" },
      { name: "second-project", rootPath: "C:/code/second" },
    ]);
  });

  it("projects architecture responses onto bounded high-signal sections", () => {
    const normalized = normalizeArchitectureResponse(architectureResponse);

    expect(normalized).toEqual({
      project: "example-project",
      total_nodes: 1000,
      total_edges: 2000,
      languages: architectureResponse.languages,
      packages: architectureResponse.packages.slice(0, 20),
      entry_points: architectureResponse.entry_points,
      hotspots: architectureResponse.hotspots,
      boundaries: architectureResponse.boundaries,
      layers: architectureResponse.layers,
      clusters: architectureResponse.clusters,
    });
    expect(JSON.stringify(normalized)).not.toContain("file_tree");
    expect(JSON.stringify(normalized)).not.toContain("routes");
  });

  it("normalizes search results into compact candidates", () => {
    expect(normalizeSearchResponse(searchGraphResponse)).toEqual([
      {
        qualifiedName: "com.example.booking.BookingService.cancelBooking",
        label: "Method",
        filePath: "src/main/java/com/example/booking/BookingService.java",
        line: 42,
        signature: "public BookingResponse cancelBooking(String bookingId)",
      },
    ]);
  });

  it("normalizes search response field variants", () => {
    expect(
      normalizeSearchResponse({
        semantic_results: [
          {
            qn: "com.example.booking.BookingService",
            type: "class",
            file: "src/main/java/com/example/booking/BookingService.java",
            line: 12,
          },
        ],
        matches: [
          {
            name: "com.example.booking.BookingService.bookingId",
            labels: ["Property"],
            path: "src/main/java/com/example/booking/BookingService.java",
            start_line: 18,
            signature: "private String bookingId",
          },
        ],
      }),
    ).toEqual([
      {
        qualifiedName: "com.example.booking.BookingService",
        label: "Class",
        filePath: "src/main/java/com/example/booking/BookingService.java",
        line: 12,
        signature: "",
      },
      {
        qualifiedName: "com.example.booking.BookingService.bookingId",
        label: "Field",
        filePath: "src/main/java/com/example/booking/BookingService.java",
        line: 18,
        signature: "private String bookingId",
      },
    ]);
  });

  it("preserves non-canonical search labels for output rows", () => {
    expect(
      normalizeSearchResponse({
        results: [
          {
            qualified_name: "com.example.booking.BookingService.mystery",
            label: "Procedure",
          },
        ],
      }),
    ).toEqual([
      {
        qualifiedName: "com.example.booking.BookingService.mystery",
        label: "Procedure",
        filePath: "",
        line: null,
        signature: "",
      },
    ]);
  });

  it("accepts Goldeneye file nodes with nullable source locations", () => {
    expect(
      normalizeSearchResponse({
        results: [
          {
            qualified_name: "file::src/main/java/example/StringUtils.java",
            label: "File",
            file_path: null,
            start_line: null,
          },
        ],
      }),
    ).toEqual([
      {
        qualifiedName: "file::src/main/java/example/StringUtils.java",
        label: "File",
        filePath: "",
        line: null,
        signature: "",
      },
    ]);
  });

  it("normalizes selected symbol metadata and source", () => {
    expect(normalizeSelectedSymbol(methodSnippetResponse)).toMatchObject({
      qualifiedName: "com.example.booking.BookingService.cancelBooking",
      kind: "Method",
      filePath: "src/main/java/com/example/booking/BookingService.java",
      startLine: 42,
      endLine: 58,
      lines: 17,
      complexity: 3,
      cognitive: 4,
      signature: "public BookingResponse cancelBooking(String bookingId)",
      returnType: "BookingResponse",
      callers: 4,
      callees: 2,
      source: methodSnippetResponse.code,
    });
  });

  it("preserves non-canonical selected symbol kinds for metadata output", () => {
    expect(
      normalizeSelectedSymbol({
        qn: "com.example.booking.BookingService.reconcileBooking",
        type: "service",
        file: "src/main/java/com/example/booking/BookingService.java",
        line: 64,
        source: "source text",
      }),
    ).toMatchObject({
      qualifiedName: "com.example.booking.BookingService.reconcileBooking",
      kind: "service",
      filePath: "src/main/java/com/example/booking/BookingService.java",
      startLine: 64,
      source: "source text",
    });
  });

  it("keeps large symbols as metadata without dropping source for get commands", () => {
    const selected = normalizeSelectedSymbol(largeMethodSnippetResponse);
    expect(selected.lines).toBe(95);
    expect(selected.source).toContain("cancelBooking");
  });
});
