use std::fs;
use std::process::Command;
use std::sync::Arc;

use goldeneye_artifact::FileArtifactPersistence;
use goldeneye_discovery::FileSystemDiscovery;
use goldeneye_git::GitCommandRepository;
use goldeneye_mcp::server::Server;
use goldeneye_mcp::tools::ToolRegistry;
use goldeneye_services::{IndexRepositoryRequest, ServiceConfig, ServiceDependencies, Services};
use goldeneye_syntax::{CoreGrammarProvider, SyntaxEngine};
use goldeneye_tree_sitter_index::TreeSitterIndexExtractor;
use serde_json::{Value, json};

fn service_dependencies() -> ServiceDependencies {
    let discovery = Arc::new(FileSystemDiscovery);
    ServiceDependencies::new(
        Arc::new(FileArtifactPersistence),
        Arc::new(GitCommandRepository),
        discovery,
        Arc::new(TreeSitterIndexExtractor::new(CoreGrammarProvider)),
        Arc::new(SyntaxEngine::new(CoreGrammarProvider)),
    )
}

fn git(root: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_AUTHOR_NAME", "Goldeneye")
        .env("GIT_AUTHOR_EMAIL", "goldeneye@example.test")
        .env("GIT_COMMITTER_NAME", "Goldeneye")
        .env("GIT_COMMITTER_EMAIL", "goldeneye@example.test")
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?}");
}

#[allow(clippy::needless_pass_by_value)]
fn call(server: &Server, id: i64, arguments: Value) -> Value {
    let line = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {"name": "detect_changes", "arguments": arguments}
    })
    .to_string();
    serde_json::to_value(server.handle_line(&line).expect("response"))
        .expect("response JSON")["result"]
        .clone()
}

fn structured(result: &Value) -> &Value {
    let text: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().expect("text content"))
            .expect("JSON text");
    assert_eq!(text, result["structuredContent"]);
    &result["structuredContent"]
}

#[test]
fn registry_exposes_exact_detect_changes_contract() {
    let registry = ToolRegistry::implemented();
    let tool = registry
        .page(None)
        .expect("tools")
        .tools
        .into_iter()
        .find(|tool| tool.name == "detect_changes")
        .expect("detect_changes");
    assert_eq!(tool.title, "Detect changes");
    assert_eq!(tool.description, "Detect code changes and their impact");
    assert_eq!(
        tool.input_schema,
        json!({
            "type": "object",
            "properties": {
                "project": {"type": "string"},
                "scope": {"type": "string"},
                "depth": {"type": "integer", "default": 2},
                "base_branch": {"type": "string", "default": "main"},
                "since": {
                    "type": "string",
                    "description": "Git ref or date to compare from (e.g. HEAD~5, v0.5.0, 2026-01-01)"
                }
            },
            "required": ["project"]
        })
    );
}

#[test]
fn detect_changes_preserves_envelope_errors_precedence_and_untracked_files() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("repo");
    fs::create_dir(&root).expect("repo");
    git(&root, &["init", "-q", "-b", "main"]);
    fs::write(root.join("base.rs"), "pub fn base() {}\n").expect("base");
    git(&root, &["add", "base.rs"]);
    git(&root, &["commit", "-q", "-m", "base"]);

    let services = Services::new(
        ServiceConfig::new(temp.path().join("graph.sqlite3"), &root).with_allowed_root(temp.path()),
        service_dependencies(),
    );
    let project = services
        .index_repository(&IndexRepositoryRequest::new(&root))
        .expect("index")
        .project;
    let server = Server::new(services);

    let missing = call(&server, 1, json!({}));
    assert_eq!(missing["isError"], true);
    assert!(
        missing["content"][0]["text"]
            .as_str()
            .expect("missing text")
            .contains("missing required argument: project")
    );

    let invalid = call(
        &server,
        2,
        json!({"project": "not-indexed", "base_branch": "--output=/tmp/pwn"}),
    );
    assert_eq!(invalid["isError"], true);
    assert!(
        invalid["content"][0]["text"]
            .as_str()
            .expect("invalid text")
            .contains("invalid characters")
    );

    let bad_branch = call(
        &server,
        3,
        json!({"project": project, "base_branch": "no-such-branch-xyz"}),
    );
    assert_eq!(bad_branch["isError"], true);
    let bad_body = structured(&bad_branch);
    assert_eq!(bad_body["changed_files"], json!([]));
    assert!(
        bad_body["hint"]
            .as_str()
            .expect("hint")
            .contains("no-such-branch-xyz")
    );

    fs::write(root.join("new.rs"), "pub fn new() {}\n").expect("untracked");
    let result = call(
        &server,
        4,
        json!({
            "project": project,
            "since": "HEAD",
            "base_branch": "no-such-branch-xyz",
            "depth": 99,
            "scope": "files"
        }),
    );
    assert_eq!(result["isError"], false);
    let body = structured(&result);
    assert_eq!(body["changed_files"], json!(["new.rs"]));
    assert_eq!(body["changed_count"], 1);
    assert_eq!(body["impacted_symbols"], json!([]));
    assert_eq!(body["depth"], 15);
    assert!(body.get("is_error").is_none());

    let unknown = call(&server, 5, json!({"project": "missing-project"}));
    assert_eq!(unknown["isError"], true);
    let message = unknown["content"][0]["text"].as_str().expect("error text");
    assert!(message.contains("or not indexed"));
    assert!(message.contains("hint"));
}
