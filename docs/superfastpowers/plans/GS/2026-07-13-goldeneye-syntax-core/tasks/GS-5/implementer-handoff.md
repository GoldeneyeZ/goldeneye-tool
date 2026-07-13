# GS-5 Implementer Handoff

Status: active

- Review type: final integration
- Source: `../../final-review.md`
- Reviewed range: `9c0cee8..52cb046`

## Required Fix

The overlap filter in `parse_direct_parser` counts a partial numeric ABI token
at one chunk end and the completed token in the next chunk. A single marker
split as `LANGUAGE_VERSION 1 | 4` is therefore counted as ABI `1` plus ABI
`14` and rejected by the exactly-one check.

## Acceptance Criteria

- Add a RED regression covering every split point in
  `#define LANGUAGE_VERSION 14\n`; the current implementation must fail only
  the digit split before repair.
- Defer matches that can be partial at a non-final window boundary, then count
  the physical marker exactly once when complete or at EOF.
- Continue rejecting two complete identical ABI markers.
- Preserve streaming, bounded overlap, byte hashing, ABI/symbol parsing, and
  the exact Git-byte repair unchanged.
- Run exporter focused tests, exporter `--check`, workspace gates, and fresh
  spec/code-quality reviews for the repair range.
