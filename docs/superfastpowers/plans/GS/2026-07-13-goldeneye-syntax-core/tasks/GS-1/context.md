# Context for GS-1

**Plan:** `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core.md`
**Task:** `GS-1`
**Plan commit:** `4305d0c`
**Implementation commit:** `b9dfd27`
**Reviewed range:** `b9dfd27^..b9dfd27`

## Summary

- `goldeneye-domain` now owns the shared `LanguageId` and typed `LanguageIdError`.
- `goldeneye-discovery` publicly re-exports that exact type and documents the 0.1 pre-release constructor error change.
- `goldeneye-syntax` exposes the `GrammarProvider` boundary and a six-language `CoreGrammarProvider` with pinned crate provenance and generated ABI metadata.

## Files Created

- `crates/goldeneye-domain/tests/language_id.rs`
- `crates/goldeneye-discovery/tests/domain_ids.rs`
- `crates/goldeneye-syntax/Cargo.toml`
- `crates/goldeneye-syntax/src/lib.rs`
- `crates/goldeneye-syntax/src/grammar.rs`
- `crates/goldeneye-syntax/tests/core_grammars.rs`

## Files Modified

- `Cargo.lock`
- `crates/goldeneye-domain/src/lib.rs`
- `crates/goldeneye-discovery/Cargo.toml`
- `crates/goldeneye-discovery/src/lib.rs`

## Relevant Files Inspected

- `Cargo.toml`
- `crates/goldeneye-domain/Cargo.toml`
- `crates/goldeneye-discovery/Cargo.toml`
- `crates/goldeneye-discovery/src/lib.rs`
- `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core.md`
- `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core/tasks/GS-1/task.md`

## TDD Evidence

- RED: `cargo test -p goldeneye-domain --test language_id` exited 101 because `LanguageId` and `LanguageIdError` were absent from domain.
- RED: `cargo test -p goldeneye-discovery --test domain_ids` exited 101 because discovery had no domain dependency/type identity.
- GREEN: both shared-ID integration tests passed, 2 tests each.
- Regression: `cargo test -p goldeneye-discovery` passed all unit/integration/doc suites.
- RED: `cargo test -p goldeneye-syntax --test core_grammars` exited 101 because the provider APIs were absent.
- GREEN: `cargo test -p goldeneye-syntax --test core_grammars` passed 4 tests, including valid parses for all six grammars.

## Final Verification

- `cargo fmt --check` — exit 0.
- `cargo clippy --workspace --all-targets -- -D warnings` — exit 0.
- `cargo test --workspace` — exit 0; 22 result sets, 103 tests passed, 0 failed.
- `git diff --check` — exit 0 before the implementation commit.
- Fresh post-review rerun of format, clippy, workspace tests, and diff check — all exit 0.

## Reviewer Notes

- Core IDs are returned in exact lexical order: Go, JavaScript, Python, Rust, TSX, TypeScript.
- Metadata versions exactly match the pinned manifest dependencies.
- ABI metadata comes from `Language::abi_version()` and uses checked `u32` conversion.
- No active implementation handoff.

## Review Results

- Independent spec review: checked; no findings.
- Independent code-quality review: checked; no findings.
