# Context for GS-3

**Plan:** `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core.md`
**Task:** `GS-3`
**Plan Commit SHA:** `4305d0c`

## Starting Context

- `crates/goldeneye-syntax/src/locator.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/tests/locators.rs`: starting point named by implementation plan.
- `rust
#[test]
fn every_named_node_has_unique_resolvable_locator() {
    let snapshot = rust_snapshot("fn alpha(x: i32) -> i32 { x + 1 }");
    let context = file_context("project", "src/lib.rs");
    let locators = all_named_locators(&snapshot, &context).unwrap();
    assert!(!locators.is_empty());
    let unique: HashSet<_> = locators.iter().collect();
    assert_eq!(unique.len(), locators.len());
    for locator in &locators {
        let node = resolve_locator(&snapshot, &context, locator).unwrap();
        assert_eq!(node.kind(), locator.anchor.node_kind.as_str());
        assert_eq!(node.start_byte() as u64, locator.anchor.source_span.bytes.start);
        assert_eq!(node.end_byte() as u64, locator.anchor.source_span.bytes.end);
    }
}
`: starting point named by implementation plan.
- `

- [ ] **Step 2: Write failing stale-guard tests**

Build a valid locator, mutate one guard at a time, and cover independently:

- wrong project ID;
- wrong project-relative file;
- wrong language ID;
- wrong grammar provider/name/revision/ABI;
- wrong file hash;
- wrong generation;
- invalid ancestor named index;
- ancestor kind mismatch;
- ancestor field-name mismatch;
- terminal kind mismatch;
- terminal byte range mismatch;
- terminal point span mismatch;
- terminal content hash mismatch.

Every case returns a distinct typed `: starting point named by implementation plan.
- `, cannot return a node, and never falls back to byte-only or fuzzy matching. Add a locator JSON golden/round-trip test here as an integration check over real source-derived values.

- [ ] **Step 3: Run tests and verify RED**

Run: `: starting point named by implementation plan.
- `

Expected: FAIL because locator API is undefined.

- [ ] **Step 4: Implement scope construction and typed errors**

`: starting point named by implementation plan.
- ` derives language, grammar fingerprint, file hash, and generation only from the immutable snapshot; callers cannot override them. `: starting point named by implementation plan.
- ` reconstructs the actual scope the same way and compares every field before traversing. Errors identify the exact failed guard but never include raw source bytes.

- [ ] **Step 5: Implement iterative locator construction/resolution**

Build ancestor steps from root through named children. Each step records parent-relative named-child index, expected child kind, and field name when Tree-sitter exposes one. When resolving, recover the raw child index while iterating named children before checking Tree-sitter's field name; a named index is not a raw child index. Root locator uses an empty ancestor path.

Resolution order is exactly current project/path -> snapshot language/grammar/hash/generation -> ancestor index/kind/field traversal -> terminal kind/byte+point span -> content hash. It returns `: starting point named by implementation plan.
- ` borrowed from snapshot only after every check passes.

- [ ] **Step 6: Verify locators**

Run: `: starting point named by implementation plan.
- `

Expected: all named nodes are unique/resolvable; domain JSON is portable; every scope, ancestor, terminal, and stale mutation rejects with its exact error.

- [ ] **Step 7: Commit**

`: starting point named by implementation plan.
- `bash
git add crates/goldeneye-syntax
git commit -m "[GS-3] feat: add guarded syntax node locators"
`: starting point named by implementation plan.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Implementation commit: `14f92a4` (`[GS-3] feat: add guarded syntax node locators`).
- Reviewed range: `7adce58..14f92a4`.
- Created: `crates/goldeneye-syntax/src/locator.rs`, `crates/goldeneye-syntax/tests/locators.rs`.
- Modified: `Cargo.lock`, `crates/goldeneye-syntax/Cargo.toml`, `crates/goldeneye-syntax/src/lib.rs`.
- Relevant code inspected: syntax snapshot/grammar APIs, validated domain locator identities, Tree-sitter 0.26.11 child/field APIs, GS plan, and Rust-port structural editing contract.
- RED: focused locator test exited 101 with E0432 because `LocatorError`, `all_named_locators`, `locator_scope`, and `resolve_locator` did not exist.
- GREEN: focused locator test passed 21/21; full syntax crate passed 35 integration tests (4 grammar + 10 diagnostic + 21 locator), with zero failures.
- Quality gates: `cargo fmt --all --check` passed; `cargo clippy -p goldeneye-syntax --all-targets -- -D warnings` passed.
- Final integration gates: workspace clippy passed; `cargo test --workspace` passed 139 tests across 25 suites with zero failures; `cargo build --workspace --release` passed; `git diff --check` reported no whitespace error.
- Independent spec review: checked, no findings (`spec-review.md`).
- Independent code quality review: checked, no findings (`code-quality.md`).
- No active implementer handoff.
