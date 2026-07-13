### Task 4: Add Bounded Token-Efficient Syntax Inspection

<TASK-ID>GS-4</TASK-ID>

**Files:**
- Create: `crates/goldeneye-syntax/src/inspect.rs`
- Modify: `crates/goldeneye-syntax/src/lib.rs`
- Modify: `crates/goldeneye-syntax/Cargo.toml`
- Create: `crates/goldeneye-syntax/tests/inspect.rs`
- Create: `crates/goldeneye-syntax/tests/fixtures/compact-inspection.json`

- [ ] **Step 1: Write failing inspection tests**

```rust
#[test]
fn inspection_is_deterministic_named_only_and_resolvable() {
    let snapshot = rust_snapshot("struct A { x: i32 }\nfn f() {}");
    let request = InspectRequest::default();
    let first = inspect_syntax(&snapshot, &context(), &request).unwrap();
    let second = inspect_syntax(&snapshot, &context(), &request).unwrap();
    assert_eq!(first, second);
    for view in &first.nodes {
        let locator = first.locator(view.ordinal).unwrap();
        resolve_locator(&snapshot, &context(), &locator).unwrap();
    }
}

#[test]
fn inspection_enforces_depth_node_and_preview_bounds() {
    let snapshot = deeply_nested_snapshot();
    let request = InspectRequest {
        max_depth: 2,
        max_nodes: 5,
        preview_chars: 8,
        byte_range: None,
    };
    let view = inspect_syntax(&snapshot, &context(), &request).unwrap();
    assert!(view.nodes.len() <= 5);
    assert!(view.nodes.iter().all(|node| node.depth <= 2));
    assert!(view.nodes.iter().all(|node| node
        .preview
        .as_ref()
        .is_none_or(|preview| preview.chars().count() <= 8)));
    assert!(view.truncated);
    assert!(view.total_named_nodes_seen >= view.nodes.len());
}

#[test]
fn non_root_range_keeps_one_base_path_and_resolvable_deltas() {
    let snapshot = rust_snapshot("fn outer() { if true { let answer = 42; } }");
    let range = byte_span_of(&snapshot, "let_declaration");
    let request = InspectRequest { byte_range: Some(range), ..InspectRequest::default() };
    let view = inspect_syntax(&snapshot, &context(), &request).unwrap();
    assert!(!view.base_ancestor_path.is_empty());
    for node in &view.nodes {
        let locator = view.locator(node.ordinal).unwrap();
        resolve_locator(&snapshot, &context(), &locator).unwrap();
    }
}

#[test]
fn invalid_range_and_over_cap_requests_are_typed_errors() {
    let snapshot = rust_snapshot("fn f() {}");
    assert!(matches!(
        inspect_syntax(&snapshot, &context(), &request_with_end(snapshot.source().len() as u64 + 1)),
        Err(InspectError::RangeOutOfBounds { .. })
    ));
    assert!(matches!(
        inspect_syntax(&snapshot, &context(), &request_with_max_depth(33)),
        Err(InspectError::LimitExceeded { field: "max_depth", .. })
    ));
}

#[test]
fn compact_serialization_has_stable_shape_and_budget() {
    let inspection = inspect_syntax(&wide_200_node_snapshot(), &context(), &InspectRequest::default()).unwrap();
    let encoded = serde_json::to_vec(&inspection).unwrap();
    assert!(encoded.len() <= 32_768, "{} bytes", encoded.len());
    let small = inspect_syntax(&rust_snapshot("fn f() {}"), &context(), &InspectRequest::default()).unwrap();
    assert_eq!(
        serde_json::to_string(&small).unwrap(),
        include_str!("fixtures/compact-inspection.json").trim()
    );
}
```

Add focused preview cases over raw bytes for `é`, an emoji, newline, backslash, and `0xff`. Escaping happens as indivisible atoms: a cap may omit `\\n` or `\\\\`, but can never return half an escape. Assert no raw newline, valid UTF-8 output, replacement character only for invalid source bytes, and scalar count at or below the requested cap.

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test -p goldeneye-syntax --test inspect`

Expected: FAIL because inspection API is undefined.

- [ ] **Step 3: Implement bounded compact request/result types**

Defaults and hard caps:

- `max_depth = 4`, hard cap `32`;
- `max_nodes = 200`, hard cap `1000`;
- `preview_chars = 0` by default, hard cap `256` Unicode scalar values;
- optional domain `ByteSpan` must be ordered and lie inside source.

Reject values beyond hard caps with typed errors rather than silently clamping.

`SyntaxInspection` contains one shared `LocatorScope`, one base ancestor path for a ranged subtree, and flat preorder nodes. A node contains only ordinal, parent ordinal, depth, parent-relative named-child index/field, kind, byte/point span, content hash, named-child count, and optional preview. It never repeats scope or full ancestor prefixes. `SyntaxInspection::locator(ordinal)` reconstructs a full domain `NodeLocator` by following parent ordinals plus the shared base path. Serde uses documented compact field names; the golden test makes wire drift explicit.

Add test-only `serde_json` to `goldeneye-syntax` for the golden and encoded-size gate.

Preview decoding is lossy UTF-8, then escaped into indivisible single-line atoms and bounded by Unicode scalar values without splitting an atom. Hashes/spans always use original bytes.

- [ ] **Step 4: Implement iterative preorder inspection**

Traverse named nodes only; prune outside optional byte range and beyond depth. For a ranged request, retain one shared base path sufficient to reconstruct every emitted locator. Validate parent ordinals form an acyclic earlier-node chain. Count nodes seen even after result cap so `truncated` and total remain truthful. Never recurse on call stack.

- [ ] **Step 5: Verify inspection**

Run: `cargo fmt --check && cargo clippy -p goldeneye-syntax --all-targets -- -D warnings && cargo test -p goldeneye-syntax`

Expected: deterministic, bounded, range-filtered, multibyte/escape/invalid-byte preview, compact JSON golden/budget, and reconstructed-locator resolution tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/goldeneye-syntax
git commit -m "[GS-4] feat: inspect syntax with bounded context"
```
