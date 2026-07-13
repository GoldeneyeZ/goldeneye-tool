# GD-6 Spec Review

- Result: checked
- Reviewed implementation commit: `8bd2190`

## Evidence Reviewed

- Exact implementation diff `8bd2190^..8bd2190`
- GD-6 plan and task package requirements
- `tools/export_upstream_languages.py`
- `crates/goldeneye-discovery/data/languages.tsv`
- `crates/goldeneye-discovery/src/language.rs`
- `crates/goldeneye-discovery/tests/language_parity.rs`
- RED/GREEN and gate evidence in `context.md`

## Compliance Notes

- Generated-data hygiene test catches terminal TSV tabs and passes on the regenerated artifact.
- Exporter writes the exact `-` sentinel for every empty extension, filename, and compound-extension list.
- Parser maps only an exact `-` field to an empty list and rejects sentinel/data mixtures for every mapping kind.
- Regenerated data preserves the audited `160/239/33/1` counts and reproduces byte-for-byte from pinned upstream.
- Workspace tests, formatting, clippy, and the phase whitespace gate pass; no unrelated production behavior changed.
