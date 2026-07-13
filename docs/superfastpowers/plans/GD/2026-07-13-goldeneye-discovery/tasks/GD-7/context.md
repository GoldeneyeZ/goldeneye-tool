# Context for GD-7

**Plan:** `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery.md`
**Task:** `GD-7`
**Commit SHA:** `24c027f`

## Starting Context

- `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/final-review.md`: authoritative 2 Critical + 3 Important findings.
- `crates/goldeneye-discovery/src/ignore_rules.rs`: incorrect combined precedence and recursive pre-scan.
- `crates/goldeneye-discovery/src/walker.rs`: safety-core, file-policy, and symlink issues.
- `.upstream/codebase-memory-mcp/src/discover/discover.c:514-610`: authoritative tier/order behavior.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete task correctly.

## Completion Updates

- Final task commit: `24c027f`
- Reviewed commit range: `c7a8f41..24c027f`
- Files created: `spec-review.md`, `code-quality.md`, `implementer-handoff.md`
- Files modified: `crates/goldeneye-discovery/src/{ignore_rules.rs,lib.rs,policy.rs,walker.rs}`, `crates/goldeneye-discovery/tests/{ignore_parity.rs,discovery_parity.rs,upstream_parity.rs}`, `crates/goldeneye-discovery/tests/fixtures/discovery/manifest.tsv`
- Additional relevant files: pinned upstream `src/discover/discover.c:514-610,653-675`
- Verification commands/results:
  - RED: `ignore_parity` reproduced root/nested/info Git-tier conflicts; `discovery_parity` reproduced safety-core and file-policy defects; `upstream_parity` reproduced recursive pre-scan and link-option surface defects.
  - GREEN: `cargo test -p goldeneye-discovery` — 49 passed, 0 failed.
  - GREEN: `cargo clippy -p goldeneye-discovery --all-targets -- -D warnings` — exit 0.
  - GREEN: `cargo fmt --all -- --check` — exit 0.
  - FULL GATE: format, workspace clippy with warnings denied, workspace tests, release build, and diff check — exit 0.

## Implementer Summary

- Replaced recursive whole-repository ignore discovery with directory-scoped lazy caches loaded only when a directory is visited or directly queried.
- Split project Git, nested Git, global Git, and custom `.cbmignore` decisions so project/nested Git ignores are terminal and custom negation clears only the global candidate.
- Enforced the four non-negatable safety directories before custom whitelist recovery and file policies before all ignore matching.
- Removed `DiscoveryOptions.follow_symlinks`; all symlink/reparse entries are recorded and skipped without target metadata reads.
- Expanded frozen, pinned-upstream manifest coverage for all GD-7 precedence, safety, policy, and link cases.
