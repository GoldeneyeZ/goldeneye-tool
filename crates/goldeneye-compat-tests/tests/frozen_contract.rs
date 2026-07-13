use goldeneye_compat_tests::{normalize, read_jsonl, run_jsonl, workspace_root};
use serde_json::json;

#[test]
fn goldeneye_matches_frozen_foundation_contract() {
    let root = workspace_root();
    let actual =
        run_jsonl(&root.join("tests/fixtures/mcp/foundation.jsonl")).expect("run Goldeneye");
    let expected = read_jsonl(&root.join("tests/fixtures/mcp/foundation.expected.jsonl"))
        .expect("read expected responses");

    assert_eq!(normalize(actual), normalize(expected));
}

#[test]
fn normalization_changes_only_server_build_version() {
    let value = json!({
        "jsonrpc": "2.0",
        "id": "request-id",
        "result": {
            "protocolVersion": "2025-11-25",
            "serverInfo": {
                "name": "codebase-memory-mcp",
                "version": "0.1.0",
                "build": "preserved"
            },
            "version": "also-preserved"
        }
    });

    assert_eq!(
        normalize(vec![value]),
        vec![json!({
            "jsonrpc": "2.0",
            "id": "request-id",
            "result": {
                "protocolVersion": "2025-11-25",
                "serverInfo": {
                    "name": "codebase-memory-mcp",
                    "version": "<normalized>",
                    "build": "preserved"
                },
                "version": "also-preserved"
            }
        })]
    );
}

#[test]
fn normalization_preserves_non_string_server_version() {
    let value = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "serverInfo": {
                "name": "codebase-memory-mcp",
                "version": 17
            }
        }
    });

    assert_eq!(normalize(vec![value.clone()]), vec![value]);
}
