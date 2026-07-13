# Implementer Handoff for GS-1

- Status: active
- Review type: final integration
- Source: `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core/final-review.md`
- Reviewed range: `9c0cee8..6853e05`

## Required Fix

Core grammar provider metadata and its test duplicate package/version strings,
but neither is checked against the exact grammar dependency pins in
`crates/goldeneye-syntax/Cargo.toml`. A dependency bump can therefore leave
snapshots advertising stale provenance while tests remain green.

## Acceptance Criteria

- Add a RED regression that fails when any of the five exact Cargo dependency
  pins disagrees with provider metadata for the six runtime grammar IDs.
- Keep explicit assertions for Go, JavaScript, Python, Rust, TypeScript, and
  TSX; TypeScript and TSX must both map to the same pinned package.
- Use one checked source of truth or parse/check the exact manifest pins.
- Run focused and workspace gates, then obtain fresh independent spec and code
  quality reviews for the repaired range.
