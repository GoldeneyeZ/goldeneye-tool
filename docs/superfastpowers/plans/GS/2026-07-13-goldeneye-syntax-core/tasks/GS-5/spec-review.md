# GS-5 Spec Review

Result: checked

- Reviewed: 2026-07-13 Europe/Paris
- Reviewed range: `bf6172d..cd44ef4`
- Repair commit: `cd44ef4` (`[GS-5] fix: parse ABI across digit boundaries`)
- Independent reviewer: `/root/gs5_abi_boundary_repair/gs5_repair_reviewer`
- Verdict: CHECKED; no remaining GS-5 specification finding.

## Scope and Evidence

- Read the GS-5 task, context, active implementer handoff, prior spec review,
  plan progression, and goal-level failed final-integration review, then
  inspected the actual committed range. The range changes only
  `tools/export_grammar_lock.py` and `tools/test_export_grammar_lock.py` (28
  insertions, 8 deletions); `git diff --check` exits 0.
- Independently executed the `bf6172d` and `cd44ef4` exporter implementations
  in memory against every interior split of
  `b"#define LANGUAGE_VERSION 14\n"` plus a valid exported symbol. The marker
  is 28 bytes, so the probe exercised all 27 interior split points. The base
  failed only at split 26 (`LANGUAGE_VERSION 1 | 4`) with the expected
  exactly-one-marker error; the repaired head passed all 27.
- Fresh focused exporter tests pass 7/7 with no failures, errors, or skips.
  The suite includes the exhaustive all-boundary regression, duplicate
  identical-marker rejection, and marker-at-EOF/no-trailing-newline coverage
  (`tools/test_export_grammar_lock.py:86`).
- Additional head probes covered digit continuation over one and two chunk
  boundaries, an intervening empty chunk, a non-digit next byte, and final EOF.
  Continued `14 | 5` input was parsed as the complete numeric token `145` and
  rejected as unsupported rather than prematurely accepting ABI 14; valid
  three-chunk `1 | 4 | \n`, non-digit-lookahead, and EOF cases accepted ABI 14.
  Every probe's digest equaled SHA-256 over the concatenated original chunks.
- Accepted the recorded real exporter `--check` evidence in `context.md`: the
  committed lock was reproduced byte-for-byte. Per review scope, the 1.29 GB
  Git verify/sync gates were not repeated. A direct protected-surface diff
  confirms `grammars/full-pack.toml`, Cargo files, all Rust, `crates/`, and
  `xtask` are byte-identical between the review endpoints.

## Requirement Trace

1. The regression iterates `range(1, len(marker))`, which is exactly all 27
   interior split points, and supplies a valid `tree_sitter_fixture` symbol.
   Independent RED/GREEN evidence proves split 26 is the sole base failure and
   is repaired at the reviewed head.
2. `parse_direct_parser` now requires a non-digit after the captured ABI with
   `(?!\d)`. It prefetches at most the next non-empty chunk and appends only its
   first byte to the regex search window; EOF supplies no lookahead byte
   (`tools/export_grammar_lock.py:416`, `tools/export_grammar_lock.py:425`).
3. A match is counted only when the completed digit group's end satisfies
   `overlap_len < match.end(1) <= physical_end`. Thus a partial token that
   consumes digit lookahead is deferred, a completed token whose physical end
   newly enters the current window is counted exactly once, and a match wholly
   inside overlap is not recounted (`tools/export_grammar_lock.py:431`).
4. Byte count and SHA-256 state are updated only from the current original
   chunk. The lookahead byte participates solely in matching. The parser keeps
   bounded overlap, the current chunk, and at most one prefetched chunk; it does
   not load a full parser (`tools/export_grammar_lock.py:427`).
5. Duplicate identical markers still produce two completed physical matches
   and fail the exactly-one check. The parser's existing contract permits a
   completed marker at final EOF, and the committed no-newline regression
   verifies that behavior.
6. The repair leaves exact Git-byte export/materialization, the generated lock,
   Rust verification/locking, Cargo metadata, and `xtask` unchanged.

## Findings

No active specification findings. The final-integration ABI digit-boundary
finding is closed by `cd44ef4`.
