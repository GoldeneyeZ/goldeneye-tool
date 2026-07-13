use std::str::FromStr;

use goldeneye_domain::{
    AncestorStep, ByteSpan, ContentHash, FileContext, Generation, GrammarFingerprint, LanguageId,
    LocatorScope, NodeAnchor, NodeLocator, ProjectId, ProjectRelativePath, SourcePoint, SourceSpan,
};

#[test]
fn content_hash_is_lowercase_blake3_and_round_trips_as_a_json_string() {
    let hash = ContentHash::of(b"hello");
    let expected = "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f";

    assert_eq!(hash.to_string(), expected);
    assert_eq!(hash.to_string().len(), 64);
    assert_eq!(ContentHash::from_str(expected).unwrap(), hash);
    assert_eq!(
        serde_json::to_string(&hash).unwrap(),
        format!("\"{expected}\"")
    );
    assert_eq!(
        serde_json::from_str::<ContentHash>(&format!("\"{expected}\"")).unwrap(),
        hash
    );

    assert!(ContentHash::from_str(&expected.to_uppercase()).is_err());
    assert!(ContentHash::from_str("abcd").is_err());
    assert!(serde_json::from_str::<ContentHash>("[0,1,2]").is_err());
}

#[test]
fn typed_offsets_use_u64_and_validate_ordering() {
    let generation = Generation::new(u64::MAX);
    let bytes = ByteSpan::new(3, u64::MAX).unwrap();
    let start = SourcePoint::new(2, 5);
    let end = SourcePoint::new(3, 0);
    let span = SourceSpan::new(bytes, start, end).unwrap();

    assert_eq!(generation.value(), u64::MAX);
    assert_eq!(bytes.start, 3);
    assert_eq!(bytes.end, u64::MAX);
    assert_eq!(start.row, 2);
    assert_eq!(start.column_bytes, 5);
    assert_eq!(span.bytes, bytes);
    assert_eq!(
        serde_json::to_string(&generation).unwrap(),
        u64::MAX.to_string()
    );

    assert!(ByteSpan::new(4, 3).is_err());
    assert!(SourceSpan::new(ByteSpan::new(0, 1).unwrap(), end, start).is_err());
    assert!(serde_json::from_str::<ByteSpan>(r#"{"start":4,"end":3}"#).is_err());
}

#[test]
fn project_relative_path_accepts_unicode_slashes_and_rejects_unsafe_forms() {
    let valid = ProjectRelativePath::new("src/été/東京.rs").unwrap();
    assert_eq!(valid.as_str(), "src/été/東京.rs");
    assert_eq!(
        serde_json::to_string(&valid).unwrap(),
        r#""src/été/東京.rs""#
    );
    assert_eq!(
        serde_json::from_str::<ProjectRelativePath>(r#""src/été/東京.rs""#).unwrap(),
        valid
    );

    for invalid in [
        "",
        "/src/lib.rs",
        "C:/src/lib.rs",
        "c:src/lib.rs",
        "src\\lib.rs",
        "src//lib.rs",
        "src/",
        "./src.rs",
        "src/./lib.rs",
        "../src.rs",
        "src/../lib.rs",
        "src/\0lib.rs",
    ] {
        assert!(
            ProjectRelativePath::new(invalid).is_err(),
            "accepted invalid path {invalid:?}"
        );
        assert!(serde_json::from_str::<ProjectRelativePath>(&format!("\"{invalid}\"")).is_err());
    }
}

#[test]
fn locator_identity_has_an_exact_validated_json_shape() {
    let file_hash = ContentHash::from_str(&"11".repeat(32)).unwrap();
    let node_hash = ContentHash::from_str(&"22".repeat(32)).unwrap();
    let locator = NodeLocator::new(
        LocatorScope::new(
            FileContext::new(
                ProjectId::new("project-α").unwrap(),
                ProjectRelativePath::new("src/été.rs").unwrap(),
            ),
            LanguageId::new("rust").unwrap(),
            GrammarFingerprint::new("rust-crate", "rust", "tree-sitter-rust@0.24.2", 15).unwrap(),
            file_hash,
            Generation::new(7),
        ),
        NodeAnchor::new(
            vec![AncestorStep::new("function_item", 2, Some("body".into())).unwrap()],
            "identifier",
            SourceSpan::new(
                ByteSpan::new(3, 7).unwrap(),
                SourcePoint::new(0, 3),
                SourcePoint::new(0, 7),
            )
            .unwrap(),
            node_hash,
        )
        .unwrap(),
    );
    let golden = concat!(
        r#"{"scope":{"file":{"project_id":"project-α","relative_path":"src/été.rs"},"language_id":"rust","grammar":{"provider":"rust-crate","grammar":"rust","revision":"tree-sitter-rust@0.24.2","abi":15},"file_hash":""#,
        "1111111111111111111111111111111111111111111111111111111111111111",
        r#"","generation":7},"anchor":{"ancestor_path":[{"node_kind":"function_item","named_child_index":2,"field_name":"body"}],"node_kind":"identifier","source_span":{"bytes":{"start":3,"end":7},"start":{"row":0,"column_bytes":3},"end":{"row":0,"column_bytes":7}},"content_hash":""#,
        "2222222222222222222222222222222222222222222222222222222222222222",
        r#""}}"#,
    );

    assert_eq!(serde_json::to_string(&locator).unwrap(), golden);
    assert_eq!(
        serde_json::from_str::<NodeLocator>(golden).unwrap(),
        locator
    );

    let invalid_path = golden.replace("src/été.rs", "../secret.rs");
    assert!(serde_json::from_str::<NodeLocator>(&invalid_path).is_err());
    let invalid_kind = golden.replace("\"identifier\"", "\"\"");
    assert!(serde_json::from_str::<NodeLocator>(&invalid_kind).is_err());
}

#[test]
fn project_and_language_ids_deserialize_through_validation() {
    let project = ProjectId::new("sample").unwrap();
    let language = LanguageId::new("rust").unwrap();

    assert_eq!(serde_json::to_string(&project).unwrap(), r#""sample""#);
    assert_eq!(serde_json::to_string(&language).unwrap(), r#""rust""#);
    assert_eq!(
        serde_json::from_str::<ProjectId>(r#""sample""#).unwrap(),
        project
    );
    assert_eq!(
        serde_json::from_str::<LanguageId>(r#""rust""#).unwrap(),
        language
    );
    assert!(serde_json::from_str::<ProjectId>(r#"""#).is_err());
    assert!(serde_json::from_str::<LanguageId>(r#"""#).is_err());
}
