### Task 3: Implement Stable Named-Node Locators

<TASK-ID>GS-3</TASK-ID>

**Files:**
- Create: `crates/goldeneye-syntax/src/locator.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Create: `crates/goldeneye-syntax/tests/locators.rs`

- [ ] **Step 1: Write failing locator coverage tests**

```rust
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
```

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

Every case returns a distinct typed `LocatorError`, cannot return a node, and never falls back to byte-only or fuzzy matching. Add a locator JSON golden/round-trip test here as an integration check over real source-derived values.

- [ ] **Step 3: Run tests and verify RED**

Run: `cargo test -p goldeneye-syntax --test locators`

Expected: FAIL because locator API is undefined.

- [ ] **Step 4: Implement scope construction and typed errors**

`locator_scope(snapshot, file_context)` derives language, grammar fingerprint, file hash, and generation only from the immutable snapshot; callers cannot override them. `resolve_locator(snapshot, current_file_context, locator)` reconstructs the actual scope the same way and compares every field before traversing. Errors identify the exact failed guard but never include raw source bytes.

- [ ] **Step 5: Implement iterative locator construction/resolution**

Build ancestor steps from root through named children. Each step records parent-relative named-child index, expected child kind, and field name when Tree-sitter exposes one. When resolving, recover the raw child index while iterating named children before checking Tree-sitter's field name; a named index is not a raw child index. Root locator uses an empty ancestor path.

Resolution order is exactly current project/path -> snapshot language/grammar/hash/generation -> ancestor index/kind/field traversal -> terminal kind/byte+point span -> content hash. It returns `tree_sitter::Node` borrowed from snapshot only after every check passes.

- [ ] **Step 6: Verify locators**

Run: `cargo test -p goldeneye-syntax --test locators`

Expected: all named nodes are unique/resolvable; domain JSON is portable; every scope, ancestor, terminal, and stale mutation rejects with its exact error.

- [ ] **Step 7: Commit**

```bash
git add crates/goldeneye-syntax
git commit -m "[GS-3] feat: add guarded syntax node locators"
```
