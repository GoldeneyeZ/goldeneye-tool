# Context for GD-2

**Plan:** `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery.md`
**Task:** `GD-2`
**Commit SHA:** Pending until task completion. If review fixes add commits, update latest task commit and reviewed range below.

## Starting Context

- `tools/export_upstream_languages.py`: starting point named by implementation plan.
- `crates/goldeneye-discovery/data/languages.tsv`: starting point named by implementation plan.
- `crates/goldeneye-discovery/src/language.rs`: starting point named by implementation plan.
- `crates/goldeneye-discovery/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-discovery/tests/language_parity.rs`: starting point named by implementation plan.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete task correctly.

## Completion Updates

- Final task commit: pending implementation commit
- Reviewed commit range: pending implementation commit
- Files created: `.gitattributes`, `tools/export_upstream_languages.py`, `crates/goldeneye-discovery/data/languages.tsv`, `crates/goldeneye-discovery/src/language.rs`, `crates/goldeneye-discovery/tests/language_parity.rs`
- Files modified: `crates/goldeneye-discovery/src/lib.rs`
- Additional relevant files: audited `.upstream/codebase-memory-mcp/internal/cbm/cbm.h` and `.upstream/codebase-memory-mcp/src/discover/language.c` at `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`
- RED evidence: `cargo test -p goldeneye-discovery --test language_parity` exited 101 because `languages.tsv` and `LanguageRegistry` did not exist; focused Git LF-policy test exited 101 before `.gitattributes` existed.
- Verification: exporter exited 0 and produced 160 LF-only data rows; `cargo fmt -p goldeneye-discovery -- --check`, `cargo clippy -p goldeneye-discovery --all-targets -- -D warnings`, `cargo test -p goldeneye-discovery` (3 unit + 8 parity tests), and `git diff --check` exited 0.

