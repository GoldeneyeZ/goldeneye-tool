# GD-2 Spec Review

- Result: checked
- Reviewed commit: `72dd442f8607b7b83fce9ba36f666f460d3d2062`
- Evidence reviewed: GD-2 task package; committed exporter, TSV, Rust registry, public export, parity tests, and `.gitattributes`; audited upstream `cbm.h` and `language.c`; fresh exporter reproduction and package verification.

## Notes

- Exporter derives enum order from `CBM_LANG_GO` through the item before `CBM_LANG_COUNT`, parses all three named upstream tables plus compound entries, rejects any count other than `160/239/33/1`, and emits the required UTF-8 TSV header in enum order.
- Checked-in TSV contains 160 rows, 239 extensions, 33 exact filenames, one compound extension, exact repository/commit/MIT provenance, and enforced LF checkout semantics.
- `LanguageRegistry` uses `include_str!` plus `OnceLock`, required `OsString` maps and longest-first compounds, display metadata, and required override -> filename -> compound -> last-extension precedence with case-sensitive keys.
- Tests cover audited counts, representative extension/exact/hidden/compound mappings, unknown and case-sensitive inputs, override priority, provenance/LF, and byte-for-byte exporter reproduction with an intentional no-upstream CI path.
- No missing, extra, or misunderstood GD-2 behavior found.
