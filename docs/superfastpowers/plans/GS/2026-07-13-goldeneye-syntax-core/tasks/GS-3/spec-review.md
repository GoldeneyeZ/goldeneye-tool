# GS-3 Spec Review

Result: checked

Reviewed: 2026-07-13
Reviewed range: `7adce58..14f92a4`
Independent reviewer: `/root/gs_3_worker/gs3_spec_review`
Independent verdict: CHECKED; no GS-3 specification deviation found.

## Evidence Reviewed

- Task contract in `task.md` and locator/editing requirements in the GS plan and Rust-port design.
- Actual committed implementation in `crates/goldeneye-syntax/src/locator.rs`, public exports in `src/lib.rs`, dependency changes, and `tests/locators.rs`.
- Fresh `cargo test -p goldeneye-syntax --test locators`: 21 passed, 0 failed.
- Static audit: explicit `LocatorError` variants, iterative stack traversal, zero `unsafe` tokens, checked `usize` conversions, and no recursive locator traversal.

## Compliance Notes

- `locator_scope` derives language, canonical grammar fingerprint, file hash, and generation from `SyntaxSnapshot`; only project/path come from the current `FileContext` (`locator.rs:62`).
- `resolve_locator` checks project/path, snapshot language and each grammar fingerprint component, file hash, and generation before ancestry traversal; then checks ancestor index/kind/field, terminal kind/full byte-and-point span, and exact raw-byte content hash (`locator.rs:110`, `locator.rs:153`). It has no byte-only or fuzzy fallback.
- Construction is iterative preorder. Each named child step stores its parent-relative named index, kind, and Tree-sitter field. Resolution iterates raw children to recover the raw child index before querying `field_name_for_child` (`locator.rs:79`, `locator.rs:197`, `locator.rs:223`). Root ancestry is empty.
- All Tree-sitter byte, point, and raw child index conversions are checked. Source slicing uses `get`; all failures are typed and error messages contain no raw source bytes (`locator.rs:249`, `locator.rs:274`). No `unsafe` is present.
- Tests cover every named node's locator uniqueness/resolution, root behavior, source-derived JSON shape/round-trip, all 17 independently typed scope/ancestor/terminal guard failures, raw-child field recovery, no-fallback behavior, and raw-source-safe error text (`tests/locators.rs:68`, `tests/locators.rs:123`, `tests/locators.rs:190`, `tests/locators.rs:320`, `tests/locators.rs:350`, `tests/locators.rs:371`, `tests/locators.rs:436`).
- Changes stay within GS-3. The `serde_json` dev dependency is required by the requested locator JSON integration test; no GS-4 inspection API was started.

No missing, extra, or misunderstood requirement found.
