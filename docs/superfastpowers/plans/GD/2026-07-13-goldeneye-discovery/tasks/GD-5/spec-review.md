# GD-5 Spec Review

- Result: checked
- Reviewed implementation commit: `2e9f379`

## Evidence Reviewed

- `crates/goldeneye-discovery/tests/upstream_parity.rs`
- `crates/goldeneye-discovery/tests/fixtures/discovery/manifest.tsv`
- `THIRD_PARTY.md`
- audited upstream source/tests at `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`
- RED and GREEN task-test evidence in `context.md`
- complete discovery gate results in `context.md`

## Compliance Notes

- Frozen fixture covers root/nested `.gitignore`, explicit global ignore,
  root/nested `.cbmignore` negation, always/fast directory and file policies,
  supported/unsupported/exact/compound/hidden/Unicode/spaced paths, symlink,
  and oversized file behavior.
- Manifest contains 75 cited rows: 25 each for Full, Moderate, and Fast.
- Replay compares exact mode, membership, ordering, language ID, file size,
  exclusion reason, excluded directories, ignored details, and ignored total.
- Normalization changes path separators only and removes platform permission
  warnings; Windows symlink privilege handling matches cited upstream skip.
- Legal ledger records `ignore 0.4.28`, its 15 normal transitive dependencies,
  source links, exact locked licenses, and MIT-derived TSV provenance/notice.
- No production behavior or unrelated files changed.
