export const searchGraphResponse = {
  results: [
    {
      qualified_name: "com.example.booking.BookingService.cancelBooking",
      label: "Method",
      file_path: "src/main/java/com/example/booking/BookingService.java",
      start_line: 42,
      signature: "public BookingResponse cancelBooking(String bookingId)",
    },
  ],
};

export const methodSnippetResponse = {
  qualified_name: "com.example.booking.BookingService.cancelBooking",
  label: "Method",
  file_path: "src/main/java/com/example/booking/BookingService.java",
  start_line: 42,
  end_line: 58,
  lines: 17,
  complexity: 3,
  cognitive: 4,
  signature: "public BookingResponse cancelBooking(String bookingId)",
  return_type: "BookingResponse",
  callers: 4,
  callees: 2,
  code: [
    "public BookingResponse cancelBooking(String bookingId) {",
    "    Booking booking = resolveActiveBooking(bookingId);",
    "    booking.cancel();",
    "    return BookingResponse.from(booking);",
    "}",
  ].join("\n"),
};

export const largeMethodSnippetResponse = {
  ...methodSnippetResponse,
  qualified_name: "com.example.booking.BookingService.reconcileBooking",
  lines: 95,
  complexity: 12,
  cognitive: 21,
  callers: 12,
  callees: 9,
};

export const inboundTraceResponse = {
  function: "com.example.booking.BookingService.cancelBooking",
  direction: "inbound",
  mode: "calls",
  callers: [
    {
      name: "cancelBooking",
      qualified_name: "com.example.booking.BookingController.cancelBooking",
      hop: 1,
      file_path: "src/main/java/com/example/booking/BookingController.java",
      start_line: 31,
    },
  ],
};

export const outboundTraceResponse = {
  function: "com.example.booking.BookingService.cancelBooking",
  direction: "outbound",
  mode: "calls",
  callees: [
    {
      name: "findActiveBooking",
      qualified_name: "com.example.booking.BookingRepository.findActiveBooking",
      hop: 1,
      file_path: "src/main/java/com/example/booking/BookingRepository.java",
      start_line: 73,
    },
  ],
};

export const legacyInboundTraceResponse = {
  paths: [
    {
      caller: "com.example.booking.BookingController.cancelBooking",
      callee: "com.example.booking.BookingService.cancelBooking",
      file_path: "src/main/java/com/example/booking/BookingController.java",
      start_line: 31,
    },
  ],
};

export const architectureResponse = {
  project: "example-project",
  total_nodes: 1000,
  total_edges: 2000,
  languages: [{ name: "TypeScript", files: 20 }],
  packages: Array.from({ length: 21 }, (_, index) => ({ name: `package-${index}` })),
  entry_points: [{ qualified_name: "src.main" }],
  hotspots: [{ qualified_name: "src.hotspot" }],
  boundaries: [{ name: "adapter" }],
  layers: [{ name: "cli" }],
  clusters: [{ id: 1, label: "runtime" }],
  file_tree: Array.from({ length: 500 }, (_, index) => `src/file-${index}.ts`),
  routes: Array.from({ length: 50 }, (_, index) => ({ path: `/route-${index}` })),
};

export const statusResponse = {
  project: "example-project",
  indexed: true,
  symbols: 423,
};
