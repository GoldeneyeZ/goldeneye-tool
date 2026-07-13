# Context for GD-6

**Plan:** `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery.md`
**Task:** `GD-6`
**Commit SHA:** Pending until task completion. If review fixes add commits, update latest task commit and reviewed range below.

## Starting Context

- `tools/export_upstream_languages.py`: root source of trailing empty TSV cells.
- `crates/goldeneye-discovery/data/languages.tsv`: generated artifact rejected by phase-range `git diff --check`.
- `crates/goldeneye-discovery/src/language.rs`: TSV parser must recognize empty-field sentinel.
- `crates/goldeneye-discovery/tests/language_parity.rs`: regression and exact-count evidence.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete task correctly.

## Completion Updates

- Final task commit: pending review evidence commit
- Reviewed commit range: GD-6 implementation commit through final evidence commit
- Files created:
  - `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/tasks/GD-6/context.md`
  - `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/tasks/GD-6/task.md`
  - `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/tasks/GD-6/implementer-handoff.md`
- Files modified:
  - `tools/export_upstream_languages.py`
  - `crates/goldeneye-discovery/data/languages.tsv`
  - `crates/goldeneye-discovery/src/language.rs`
  - `crates/goldeneye-discovery/tests/language_parity.rs`
  - `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery.md`
  - `docs/superfastpowers/plans/GD/2026-07-13-goldeneye-discovery/plan-progression.md`
- TDD evidence:
  - RED: generated-data hygiene test failed on TSV line 5, where `go\tGo\t.go\t\t` ended in two empty fields.
  - RED: exact-sentinel and mixed-sentinel parser tests both failed because `-` was indexed as literal language data.
  - GREEN: hygiene test passed 1/1 and parser sentinel tests passed 2/2 after the exporter/parser boundary fix.
- Verification commands/results:
  - `cargo test -p goldeneye-discovery --test language_parity`: pass, 9/9
  - `cargo test --workspace`: pass
  - `cargo fmt --check`: pass
  - `cargo clippy --workspace --all-targets -- -D warnings`: pass
  - `git diff --check 13d741d`: pass; Windows line-ending conversion notices only
  - generated-data audit: exactly 160 languages, 239 extensions, 33 filenames, and 1 compound extension
  - generated-data hygiene: zero malformed rows, empty list cells, trailing-whitespace lines, or mixed sentinel fields
- Implementation notes:
  - Exporter serializes every empty list field as the exact `-` sentinel.
  - Parser maps only exact `-` to an empty list and rejects comma-separated lists that mix `-` with data.
