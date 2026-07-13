# GS-5 Implementer Handoff

- Status: active
- Review type: final integration
- Source: `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core/final-review.md`
- Reviewed range: `9c0cee8..6853e05`

## Required Fixes

1. The committed 159 source hashes describe a Windows `core.autocrlf=true`
   worktree, while the hardened exporter hashes exact LF Git blobs. Exporter
   `--check` exits 1; filesystem verify/sync accept different bytes.
2. `parse_direct_parser` deduplicates ABI marker values, so two identical
   `LANGUAGE_VERSION` markers incorrectly satisfy the exactly-one contract.

## Acceptance Criteria

- Treat raw bytes from pinned commit
  `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c` as the canonical source without
  line-ending normalization.
- Make exporter, verify, and sync consume the same byte-stable source across
  platforms. Preserve directory-source support for tiny fixtures; for the real
  pinned source, prefer explicit Git-object materialization over relying on a
  user's checkout configuration.
- Disable Git replacement resolution and reject non-regular Git modes in every
  Git-object path. Hash and copy each asset from the same immutable stream.
- Regenerate all 159 hashes and the Ada/YAML/RST core expectations from the
  exact blobs. Update provenance documentation and real-gate commands.
- Add RED regressions for an autocrlf/smudged checkout and for duplicate
  identical ABI markers, without overlap-window double-counting.
- Rerun exporter `--check`, real verify/sync, focused tests, workspace gates,
  and fresh independent spec/code-quality reviews.
