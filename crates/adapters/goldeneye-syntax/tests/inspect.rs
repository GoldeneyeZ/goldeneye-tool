#![cfg(feature = "core-grammars")]

use std::{fmt::Write as _, sync::Arc};

use goldeneye_domain::{
    ByteSpan, FileContext, Generation, LanguageId, ProjectId, ProjectRelativePath,
};
use goldeneye_syntax::{
    CoreGrammarProvider, InspectError, InspectRequest, SyntaxEngine, SyntaxInspection,
    SyntaxSnapshot, all_named_locators, inspect_syntax, resolve_locator,
};

fn rust_snapshot(source: impl Into<Arc<[u8]>>) -> SyntaxSnapshot {
    SyntaxEngine::new(CoreGrammarProvider)
        .parse(
            LanguageId::new("rust").unwrap(),
            source.into(),
            Generation::new(7),
        )
        .unwrap()
}

fn context() -> FileContext {
    FileContext::new(
        ProjectId::new("goldeneye").unwrap(),
        ProjectRelativePath::new("src/lib.rs").unwrap(),
    )
}

fn byte_span_of(snapshot: &SyntaxSnapshot, kind: &str) -> ByteSpan {
    all_named_locators(snapshot, &context())
        .unwrap()
        .into_iter()
        .find(|locator| locator.anchor.node_kind == kind)
        .unwrap_or_else(|| panic!("fixture has no {kind}"))
        .anchor
        .source_span
        .bytes
}

fn wide_snapshot() -> SyntaxSnapshot {
    let mut source = String::new();
    for index in 0..150 {
        writeln!(source, "fn item_{index}() {{}}").unwrap();
    }
    rust_snapshot(Arc::<[u8]>::from(source.into_bytes()))
}

fn root_preview(source: &[u8], preview_chars: usize) -> String {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(source));
    let request = InspectRequest {
        max_depth: 0,
        max_nodes: 1,
        preview_chars,
        byte_range: None,
        node_kinds: Vec::new(),
    };
    inspect_syntax(&snapshot, &context(), &request)
        .unwrap()
        .nodes
        .into_iter()
        .next()
        .expect("root node")
        .preview
        .expect("preview requested")
}

fn node_preview(source: &[u8], kind: &str, preview_chars: usize) -> String {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(source));
    let range = byte_span_of(&snapshot, kind);
    inspect_syntax(
        &snapshot,
        &context(),
        &InspectRequest {
            max_depth: 0,
            max_nodes: 1,
            preview_chars,
            byte_range: Some(range),
            node_kinds: Vec::new(),
        },
    )
    .unwrap()
    .nodes
    .into_iter()
    .next()
    .expect("selected node")
    .preview
    .expect("preview requested")
}

#[test]
fn inspection_is_deterministic_named_only_and_resolvable() {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(
        b"struct A { x: i32 }\nfn f() {}".as_slice(),
    ));
    let request = InspectRequest::default();
    let first = inspect_syntax(&snapshot, &context(), &request).unwrap();
    let second = inspect_syntax(&snapshot, &context(), &request).unwrap();

    assert_eq!(first, second);
    assert!(first.base_ancestor_path.is_empty());
    for view in &first.nodes {
        let locator = first.locator(view.ordinal).unwrap();
        let resolved = resolve_locator(&snapshot, &context(), &locator).unwrap();
        assert!(resolved.is_named());
        assert_eq!(resolved.kind(), view.kind);
    }
}

#[test]
fn inspection_enforces_depth_node_and_preview_bounds_truthfully() {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(
        b"fn f() { if true { while false { let answer = 42; } } }".as_slice(),
    ));
    let request = InspectRequest {
        max_depth: 2,
        max_nodes: 5,
        preview_chars: 8,
        byte_range: None,
        node_kinds: Vec::new(),
    };
    let view = inspect_syntax(&snapshot, &context(), &request).unwrap();

    assert!(view.nodes.len() <= 5);
    assert!(view.nodes.iter().all(|node| node.depth <= 2));
    assert!(view.nodes.iter().all(|node| {
        node.preview
            .as_ref()
            .is_none_or(|preview| preview.chars().count() <= 8)
    }));
    assert!(view.truncated);
    assert!(view.total_named_nodes_seen >= view.nodes.len());

    let node_limited = inspect_syntax(
        &wide_snapshot(),
        &context(),
        &InspectRequest {
            max_depth: 32,
            max_nodes: 5,
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert_eq!(node_limited.nodes.len(), 5);
    assert!(node_limited.total_named_nodes_seen > node_limited.nodes.len());
    assert!(node_limited.truncated);
}

#[test]
fn kind_filter_is_exact_result_bounded_and_resolvable() {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(
        b"fn first() { let x = 1; }\nstruct Skip;\nfn second() { let y = 2; }".as_slice(),
    ));
    let request = InspectRequest {
        max_depth: 32,
        max_nodes: 1,
        node_kinds: vec!["function_item".to_owned()],
        ..InspectRequest::default()
    };

    let inspection = inspect_syntax(&snapshot, &context(), &request).unwrap();

    assert_eq!(inspection.nodes.len(), 1);
    assert_eq!(inspection.nodes[0].kind, "function_item");
    assert_eq!(inspection.total_named_nodes_seen, 2);
    assert!(inspection.truncated);
    let locator = inspection.locator(0).unwrap();
    let resolved = resolve_locator(&snapshot, &context(), &locator).unwrap();
    assert_eq!(resolved.kind(), "function_item");
    assert_eq!(
        &snapshot.source()[resolved.start_byte()..resolved.end_byte()],
        b"fn first() { let x = 1; }"
    );
}

#[test]
fn non_root_range_keeps_one_base_path_and_resolvable_deltas() {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(
        b"fn outer() { if true { let answer = 42; } }".as_slice(),
    ));
    let range = byte_span_of(&snapshot, "let_declaration");
    let request = InspectRequest {
        byte_range: Some(range),
        ..InspectRequest::default()
    };
    let view = inspect_syntax(&snapshot, &context(), &request).unwrap();

    assert!(!view.base_ancestor_path.is_empty());
    assert_eq!(view.nodes.first().unwrap().kind, "let_declaration");
    for node in &view.nodes {
        assert!(node.span.bytes.start >= range.start);
        assert!(node.span.bytes.end <= range.end);
        assert!(node.span.bytes.end <= snapshot.source().len() as u64);
        let locator = view.locator(node.ordinal).unwrap();
        resolve_locator(&snapshot, &context(), &locator).unwrap();
    }

    let value = serde_json::to_value(&view).unwrap();
    let object = value.as_object().unwrap();
    assert_eq!(object.keys().filter(|key| key.as_str() == "s").count(), 1);
    assert_eq!(object.keys().filter(|key| key.as_str() == "b").count(), 1);
    for node in object["n"].as_array().unwrap() {
        let node = node.as_object().unwrap();
        assert!(!node.contains_key("scope"));
        assert!(!node.contains_key("ancestor_path"));
    }
}

#[test]
fn empty_ranges_use_half_open_sibling_boundaries_and_exclude_eof() {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(b"fn a() {}fn b() {}".as_slice()));
    let functions: Vec<_> = all_named_locators(&snapshot, &context())
        .unwrap()
        .into_iter()
        .filter(|locator| locator.anchor.node_kind == "function_item")
        .collect();
    assert_eq!(functions.len(), 2);
    let boundary = functions[0].anchor.source_span.bytes.end;
    assert_eq!(boundary, functions[1].anchor.source_span.bytes.start);

    let at_boundary = inspect_syntax(
        &snapshot,
        &context(),
        &InspectRequest {
            byte_range: Some(ByteSpan {
                start: boundary,
                end: boundary,
            }),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    let first = at_boundary.nodes.first().expect("second sibling selected");
    assert_eq!(first.kind, "function_item");
    assert_eq!(first.span.bytes.start, boundary);
    let resolved = resolve_locator(
        &snapshot,
        &context(),
        &at_boundary.locator(first.ordinal).unwrap(),
    )
    .unwrap();
    assert_eq!(resolved.start_byte() as u64, boundary);

    let eof = snapshot.source().len() as u64;
    let at_eof = inspect_syntax(
        &snapshot,
        &context(),
        &InspectRequest {
            byte_range: Some(ByteSpan {
                start: eof,
                end: eof,
            }),
            ..InspectRequest::default()
        },
    )
    .unwrap();
    assert!(at_eof.nodes.is_empty());
    assert_eq!(at_eof.total_named_nodes_seen, 0);
    assert!(!at_eof.truncated);
}

#[test]
fn parent_ordinals_are_preorder_acyclic_and_earlier() {
    let inspection = inspect_syntax(
        &rust_snapshot(Arc::<[u8]>::from(
            b"fn f(x: i32) { let y = x + 1; }".as_slice(),
        )),
        &context(),
        &InspectRequest::default(),
    )
    .unwrap();

    for (index, node) in inspection.nodes.iter().enumerate() {
        assert_eq!(node.ordinal as usize, index);
        match node.parent_ordinal {
            None => {
                assert_eq!(node.ordinal, 0);
                assert_eq!(node.depth, 0);
                assert!(node.named_child_index.is_none());
            }
            Some(parent) => {
                assert!(parent < node.ordinal);
                assert_eq!(inspection.nodes[parent as usize].depth + 1, node.depth);
                assert!(node.named_child_index.is_some());
            }
        }
    }
}

#[test]
fn invalid_range_and_over_cap_requests_are_typed_errors() {
    let snapshot = rust_snapshot(Arc::<[u8]>::from(b"fn f() {}".as_slice()));
    let source_len = snapshot.source().len() as u64;

    let cases = [
        (
            InspectRequest {
                max_depth: 33,
                ..InspectRequest::default()
            },
            "max_depth",
        ),
        (
            InspectRequest {
                max_nodes: 1001,
                ..InspectRequest::default()
            },
            "max_nodes",
        ),
        (
            InspectRequest {
                preview_chars: 257,
                ..InspectRequest::default()
            },
            "preview_chars",
        ),
        (
            InspectRequest {
                node_kinds: (0..33).map(|index| format!("kind_{index}")).collect(),
                ..InspectRequest::default()
            },
            "node_kinds",
        ),
    ];
    for (request, expected_field) in cases {
        assert!(matches!(
            inspect_syntax(&snapshot, &context(), &request),
            Err(InspectError::LimitExceeded { field, .. }) if field == expected_field
        ));
    }

    let reversed = InspectRequest {
        byte_range: Some(ByteSpan { start: 3, end: 2 }),
        ..InspectRequest::default()
    };
    assert!(matches!(
        inspect_syntax(&snapshot, &context(), &reversed),
        Err(InspectError::InvalidRange { start: 3, end: 2 })
    ));

    let outside = InspectRequest {
        byte_range: Some(ByteSpan {
            start: 0,
            end: source_len + 1,
        }),
        ..InspectRequest::default()
    };
    assert!(matches!(
        inspect_syntax(&snapshot, &context(), &outside),
        Err(InspectError::RangeOutOfBounds { source_len: actual, .. }) if actual == source_len
    ));
}

#[test]
fn previews_are_lossy_single_line_indivisible_scalar_bounded_atoms() {
    assert_eq!(root_preview("é".as_bytes(), 1), "é");
    assert_eq!(root_preview("🙂".as_bytes(), 1), "🙂");
    let raw_string = b"const S: &str = r\"line\nnext\";";
    assert_eq!(node_preview(raw_string, "raw_string_literal", 7), "r\"line");
    assert_eq!(
        node_preview(raw_string, "raw_string_literal", 8),
        "r\"line\\n"
    );
    assert_eq!(root_preview(b"\\", 1), "");
    assert_eq!(root_preview(b"\\", 2), "\\\\");
    assert_eq!(root_preview(b"a\n", 2), "a");
    assert_eq!(root_preview(b"a\n", 3), "a\\n");

    let valid = node_preview(
        "const S: &str = r\"é🙂\n\\\";".as_bytes(),
        "raw_string_literal",
        16,
    );
    assert!(!valid.contains('\n'));
    assert!(!valid.contains('\r'));
    assert!(!valid.contains('\u{fffd}'));
    assert!(valid.contains("é🙂\\n\\\\"));

    let invalid = root_preview(&[0xff], 1);
    assert_eq!(invalid, "\u{fffd}");
    assert!(std::str::from_utf8(invalid.as_bytes()).is_ok());

    for cap in 1..=8 {
        let preview = node_preview(
            "const S: &str = r\"é🙂\n\\\";".as_bytes(),
            "raw_string_literal",
            cap,
        );
        assert!(preview.chars().count() <= cap);
        assert!(!preview.ends_with('\u{5c}') || preview.ends_with("\\\\"));
    }
}

#[test]
fn compact_serialization_has_stable_shape_and_budget() {
    let inspection =
        inspect_syntax(&wide_snapshot(), &context(), &InspectRequest::default()).unwrap();
    assert_eq!(inspection.nodes.len(), 200);
    let encoded = serde_json::to_vec(&inspection).unwrap();
    assert!(encoded.len() <= 32_768, "{} bytes", encoded.len());

    let small: SyntaxInspection = inspect_syntax(
        &rust_snapshot(Arc::<[u8]>::from(b"fn f() {}".as_slice())),
        &context(),
        &InspectRequest::default(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&small).unwrap(),
        include_str!("fixtures/compact-inspection.json").trim()
    );
}

#[test]
fn default_request_contract_is_stable() {
    assert_eq!(
        InspectRequest::default(),
        InspectRequest {
            max_depth: 4,
            max_nodes: 200,
            preview_chars: 0,
            byte_range: None,
            node_kinds: Vec::new(),
        }
    );
}
