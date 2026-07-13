# Context for GS-4

**Plan:** `docs/superfastpowers/plans/GS/2026-07-13-goldeneye-syntax-core.md`
**Task:** `GS-4`
**Plan Commit SHA:** `4305d0c`

## Starting Context

- `crates/goldeneye-syntax/src/inspect.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/src/lib.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/Cargo.toml`: starting point named by implementation plan.
- `crates/goldeneye-syntax/tests/inspect.rs`: starting point named by implementation plan.
- `crates/goldeneye-syntax/tests/fixtures/compact-inspection.json`: starting point named by implementation plan.
- `rust
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
`: starting point named by implementation plan.
- `

Add focused preview cases over raw bytes for `: starting point named by implementation plan.
- `, an emoji, newline, backslash, and `: starting point named by implementation plan.
- `. Escaping happens as indivisible atoms: a cap may omit `: starting point named by implementation plan.
- ` or `: starting point named by implementation plan.
- `, but can never return half an escape. Assert no raw newline, valid UTF-8 output, replacement character only for invalid source bytes, and scalar count at or below the requested cap.

- [ ] **Step 2: Run tests and verify RED**

Run: `: starting point named by implementation plan.
- `

Expected: FAIL because inspection API is undefined.

- [ ] **Step 3: Implement bounded compact request/result types**

Defaults and hard caps:

- `: starting point named by implementation plan.
- `, hard cap `: starting point named by implementation plan.
- `;
- `: starting point named by implementation plan.
- `, hard cap `: starting point named by implementation plan.
- `;
- `: starting point named by implementation plan.
- ` by default, hard cap `: starting point named by implementation plan.
- ` Unicode scalar values;
- optional domain `: starting point named by implementation plan.
- ` must be ordered and lie inside source.

Reject values beyond hard caps with typed errors rather than silently clamping.

`: starting point named by implementation plan.
- ` contains one shared `: starting point named by implementation plan.
- `, one base ancestor path for a ranged subtree, and flat preorder nodes. A node contains only ordinal, parent ordinal, depth, parent-relative named-child index/field, kind, byte/point span, content hash, named-child count, and optional preview. It never repeats scope or full ancestor prefixes. `: starting point named by implementation plan.
- ` reconstructs a full domain `: starting point named by implementation plan.
- ` by following parent ordinals plus the shared base path. Serde uses documented compact field names; the golden test makes wire drift explicit.

Add test-only `: starting point named by implementation plan.
- ` to `: starting point named by implementation plan.
- ` for the golden and encoded-size gate.

Preview decoding is lossy UTF-8, then escaped into indivisible single-line atoms and bounded by Unicode scalar values without splitting an atom. Hashes/spans always use original bytes.

- [ ] **Step 4: Implement iterative preorder inspection**

Traverse named nodes only; prune outside optional byte range and beyond depth. For a ranged request, retain one shared base path sufficient to reconstruct every emitted locator. Validate parent ordinals form an acyclic earlier-node chain. Count nodes seen even after result cap so `: starting point named by implementation plan.
- ` and total remain truthful. Never recurse on call stack.

- [ ] **Step 5: Verify inspection**

Run: `: starting point named by implementation plan.
- `

Expected: deterministic, bounded, range-filtered, multibyte/escape/invalid-byte preview, compact JSON golden/budget, and reconstructed-locator resolution tests pass.

- [ ] **Step 6: Commit**

`: starting point named by implementation plan.
- `bash
git add crates/goldeneye-syntax
git commit -m "[GS-4] feat: inspect syntax with bounded context"
`: starting point named by implementation plan.

## Open Context Rule

Files above are starting points only. Inspect any additional files needed to complete the task correctly.

## Completion Updates

- Implementation commit: `49d6fd4` (`[GS-4] feat: inspect syntax with bounded context`).
- Reviewed range: `1e82f0f..49d6fd4`.
- Created: `crates/goldeneye-syntax/src/inspect.rs`, `crates/goldeneye-syntax/tests/inspect.rs`, and `tests/fixtures/compact-inspection.json`.
- Modified: `crates/goldeneye-syntax/src/lib.rs`.
- RED: the initial focused test failed with E0432 because the inspection API did not exist. The quality-repair regression then failed by selecting the preceding `block` at a zero-width sibling boundary.
- GREEN: focused inspection passed 9/9; the syntax crate passed 44 integration tests with zero failures.
- Compactness: the default 200-node fixture serializes to 31,643 bytes, below the 32,768-byte gate.
- Quality gates: `cargo fmt --all --check`, workspace clippy with `-D warnings`, and `git diff --check` passed.
- Final integration gates: `cargo test --workspace` passed 148 tests across 26 suites with zero failures; `cargo build --workspace --release` passed.
- Independent spec review: checked against final commit `49d6fd4` with no findings (`spec-review.md`).
- Independent code quality review: one Medium half-open range finding was repaired under TDD; independent recheck is checked with no remaining finding (`code-quality.md`).
- No recursion, `unsafe`, fuzzy relocation, repeated scope, or repeated ancestor prefixes were introduced.
- No active implementer handoff.
