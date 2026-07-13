### Task 2: Port Complete Language Registry

<TASK-ID>GD-2</TASK-ID>

**Files:**
- Create: `tools/export_upstream_languages.py`
- Create: `crates/goldeneye-discovery/data/languages.tsv`
- Create: `crates/goldeneye-discovery/src/language.rs`
- Modify: `crates/goldeneye-discovery/src/lib.rs`
- Create: `crates/goldeneye-discovery/tests/language_parity.rs`

- [ ] **Step 1: Write failing registry parity tests**

```rust
#[test]
fn registry_matches_audited_upstream_counts() {
    let registry = LanguageRegistry::upstream();
    assert_eq!(registry.language_count(), 160);
    assert_eq!(registry.extension_count(), 239);
    assert_eq!(registry.filename_count(), 33);
    assert_eq!(registry.compound_extension_count(), 1);
}

#[test]
fn filename_extension_and_compound_precedence_match_upstream() {
    let registry = LanguageRegistry::upstream();
    assert_eq!(registry.classify(Path::new("main.rs")).unwrap().as_str(), "rust");
    assert_eq!(registry.classify(Path::new("CMakeLists.txt")).unwrap().as_str(), "cmake");
    assert_eq!(registry.classify(Path::new(".env")).unwrap().as_str(), "dotenv");
    assert_eq!(registry.classify(Path::new("view.blade.php")).unwrap().as_str(), "blade");
    assert_eq!(registry.classify(Path::new("unknown.binary")), None);
}

#[test]
fn explicit_extension_override_wins() {
    let mut overrides = HashMap::new();
    overrides.insert(OsString::from(".mjs"), LanguageId::new("typescript").unwrap());
    let registry = LanguageRegistry::with_overrides(overrides).unwrap();
    assert_eq!(registry.classify(Path::new("index.mjs")).unwrap().as_str(), "typescript");
}
```

- [ ] **Step 2: Run test and verify RED**

Run: `cargo test -p goldeneye-discovery --test language_parity`

Expected: FAIL because registry/data do not exist.

- [ ] **Step 3: Implement reproducible exporter**

`tools/export_upstream_languages.py` accepts `--upstream` and `--output`. It must:

1. Parse `internal/cbm/cbm.h` enum order from `CBM_LANG_GO` through item before `CBM_LANG_COUNT`.
2. Parse 160 display names from `LANG_NAMES` in `src/discover/language.c`.
3. Parse 239 `EXT_TABLE` entries, 33 `FILENAME_TABLE` entries, and compound `.blade.php` entry.
4. Emit UTF-8 TSV sorted by enum order with header:
   `id<TAB>display_name<TAB>extensions<TAB>filenames<TAB>compound_extensions`.
5. Normalize IDs from `CBM_LANG_FOO_BAR` to lowercase `foo_bar`.
6. Fail unless counts equal `160/239/33/1`.
7. Include comments recording upstream repository and commit `2469ecc3a7a2f80debe296e1f17a1efcfdb9450c`.

Run:

```bash
python tools/export_upstream_languages.py --upstream .upstream/codebase-memory-mcp --output crates/goldeneye-discovery/data/languages.tsv
```

Expected: generated file contains 160 language rows and stable LF line endings.

- [ ] **Step 4: Implement immutable registry**

`LanguageRegistry` loads `include_str!("../data/languages.tsv")` once via `OnceLock` and builds:

- `HashMap<OsString, LanguageId>` for extensions;
- `HashMap<OsString, LanguageId>` for exact filenames;
- longest-first vector for compound extensions;
- `HashMap<LanguageId, LanguageSpec>` for display metadata.

Classification order: explicit override → exact filename → compound extension → last extension. Extension keys include leading dots and compare ASCII case-sensitively, matching upstream tables.

- [ ] **Step 5: Add exporter reproducibility test**

The test runs exporter against local read-only upstream checkout when present and compares bytes to checked-in TSV. When checkout is absent, it validates embedded provenance/counts and does not fail CI solely because `.upstream` is intentionally excluded.

- [ ] **Step 6: Verify registry**

Run: `cargo test -p goldeneye-discovery --test language_parity && cargo clippy -p goldeneye-discovery --all-targets -- -D warnings`

Expected: count, mapping, precedence, override, and reproducibility tests pass.

- [ ] **Step 7: Commit**

```bash
git add tools/export_upstream_languages.py crates/goldeneye-discovery
git commit -m "[GD-2] feat: port upstream language registry"
```
