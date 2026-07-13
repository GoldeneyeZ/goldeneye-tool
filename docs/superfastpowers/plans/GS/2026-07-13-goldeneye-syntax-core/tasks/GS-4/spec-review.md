# GS-4 Spec Review

Result: checked

Reviewed: 2026-07-13
Reviewed range: `1e82f0f..49d6fd4`
Independent reviewer: `/root/gs_4_worker/gs4_spec_review`
Independent verdict: CHECKED; no actionable GS-4 specification finding.

## Evidence Reviewed

- Task contract in `task.md`, committed implementation `49d6fd4`, compact golden fixture, and integration tests.
- Fresh `cargo test -p goldeneye-syntax --test inspect`: 9 passed, 0 failed.
- The default 200-node fixture serializes to 31,643 bytes, below the 32,768-byte budget.

## Compliance Notes

- `SyntaxInspection` stores one shared `LocatorScope`, one ranged base path, and compact flat node deltas; nodes never repeat a scope or full ancestor prefix (`inspect.rs:44`, `inspect.rs:73`).
- `SyntaxInspection::locator` validates earlier parent links, reconstructs the suffix from parent-relative named-child steps, appends the shared base path once, and builds an exact GS-3 `NodeLocator` (`inspect.rs:92`, `inspect.rs:372`).
- Inspection is deterministic iterative named-node preorder. Range selection retains the deepest containing named base, traversal prunes irrelevant spans, depth and node bounds are enforced, and counting continues after the result cap (`inspect.rs:192`, `inspect.rs:330`).
- Defaults and hard caps match the task. Reversed and out-of-source `ByteSpan` requests and limit excesses return typed `InspectError` variants rather than clamping (`inspect.rs:20`, `inspect.rs:140`).
- Preview bytes remain raw for hashes/spans, decode through lossy UTF-8, escape single-line atoms indivisibly, and apply the cap to escaped Unicode scalar values (`inspect.rs:475`). Tests cover `é`, emoji, physical newline, backslash, and `0xff`.
- Compact serde keys and the six-coordinate span array are documented and frozen by `compact-inspection.json` (`inspect.rs:38`, `inspect.rs:503`).

No missing, extra, or misunderstood GS-4 requirement found.
