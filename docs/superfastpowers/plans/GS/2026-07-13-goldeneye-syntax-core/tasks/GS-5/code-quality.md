# GS-5 Code-Quality Review

Result: checked

- Reviewed: 2026-07-13 Europe/Paris
- Reviewed range: `bf6172d..cd44ef4`
- Repair commit: `cd44ef4` (`[GS-5] fix: parse ABI across digit boundaries`)
- Independent reviewer: `/root/gs5_abi_boundary_repair/gs5_repair_reviewer`
- Verdict: CHECKED; no actionable code-quality finding.

## Findings

No Critical, High, Medium, or Low correctness, performance, test-quality, or
maintainability finding exists in the reviewed range.

## Open Questions and Assumptions

- Review scope is the committed repair range. The generated grammar lock,
  Git-byte exporter/materializer behavior, Cargo/Rust locking and verification,
  and `xtask` are byte-identical between the endpoints and were treated as
  protected existing behavior.
- The parser contract accepts a completed ABI marker at final EOF without a
  trailing newline. That behavior predates the repair and is now explicit in a
  focused regression.

## Quality Assessment

- **Correctness and regex semantics:** `(\d+)(?!\d)` requires proof that the
  captured numeric token is complete. The search receives only the first byte
  of the next non-empty chunk (or no byte at final EOF), and the
  `overlap_len < match.end(1) <= physical_end` filter counts only a completed
  digit group whose physical end newly enters the current window. A digit in
  lookahead extends the group beyond `physical_end`, so a valid numeric prefix
  cannot be accepted while more digits may follow
  (`tools/export_grammar_lock.py:416`, `tools/export_grammar_lock.py:433`).
- **Overlap de-duplication:** filtering on `match.end(1)` retains the existing
  exactly-once rule. A completed marker is counted in the first physical window
  containing its digit-group end; the same match is wholly inside overlap on
  the next iteration and is excluded. Two distinct complete identical markers
  still produce two values and fail closed (`tools/export_grammar_lock.py:435`).
- **Iterator and empty chunks:** the generator expression lazily skips empty
  byte chunks, which are neutral for SHA-256 and byte count, and prefetches at
  most one non-empty chunk. It does not materialize the iterable or parser
  (`tools/export_grammar_lock.py:425`).
- **Bounded memory and streaming:** the Git producer yields at most 1 MiB per
  source chunk. The consumer holds the current and one prefetched chunk, a
  1024-byte overlap, and bounded derived regex windows; the repair's buffering
  remains `O(COPY_BUFFER + overlap)` and adds no parser-sized materialization.
  The one-byte lookahead does not introduce a full-parser load
  (`tools/export_grammar_lock.py:36`, `tools/export_grammar_lock.py:185`).
- **Original-byte hashing and counting:** only `chunk` contributes to `total`
  and `hasher.update`. Neither overlap nor the lookahead-augmented search window
  contributes bytes, and the prefetched chunk is hashed exactly once when it
  becomes current (`tools/export_grammar_lock.py:427`).
- **Clarity and maintainability:** `overlap_len`, `physical_end`, and
  `search_window` make the three coordinate domains explicit. The change is
  localized to `parse_direct_parser`, preserves the existing parser/symbol
  validation flow, and adds no parallel parsing implementation or new public
  surface.
- **Test quality:** the regression is deterministic, table-like, and uses
  `subTest(split=...)` to identify any failing boundary. It covers every one of
  the 27 interior splits of the complete marker plus a valid symbol, while
  dedicated tests retain duplicate-identical-marker rejection and establish
  final-EOF behavior (`tools/test_export_grammar_lock.py:86`). Fresh focused
  execution passed 7/7, and independent adversarial probes also covered empty
  chunks, multi-boundary digits, continued `145`, non-digit lookahead, EOF,
  digest equality, and byte-count equality.
- **Unintended scope:** `bf6172d..cd44ef4` changes only
  `tools/export_grammar_lock.py` and `tools/test_export_grammar_lock.py` (28
  insertions, 8 deletions). The protected grammar lock, all Rust/Cargo files,
  `crates/`, and `xtask` have no range diff; `git diff --check` is clean.

## Summary

The repair closes the digit-boundary defect with a small, bounded lookahead
state transition and preserves the exporter's streaming and byte-identity
invariants. No code-quality follow-up is required for this range.
