use super::{
    DetectChangesOptions, GitError, GitLimits, NeverCancel, collect_history, detect_changes,
    is_trackable_file, parse_history_log, parse_hunks, parse_name_status, parse_range,
    resolve_context,
};
use std::fmt::Write as _;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

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
        .expect("run git");
    assert!(status.success(), "git {args:?}");
}

fn repository() -> TempDir {
    let directory = tempfile::tempdir().expect("temp repo");
    git(directory.path(), &["init", "-q", "-b", "main"]);
    fs::write(directory.path().join("base.rs"), "fn base() {}\n").expect("base");
    git(directory.path(), &["add", "base.rs"]);
    git(directory.path(), &["commit", "-q", "-m", "base"]);
    directory
}

#[test]
fn history_parity_filters_scores_and_skips_large_commits() {
    let mut log = String::new();
    for timestamp in 1..=3 {
        writeln!(
            log,
            "COMMIT:{timestamp}:{timestamp}\na.rs\nb.rs\nCargo.lock"
        )
        .expect("write synthetic log");
    }
    log.push_str("COMMIT:large:9\n");
    for index in 0..=20 {
        writeln!(log, "large/{index}.rs").expect("write synthetic path");
    }
    let history = parse_history_log(&log);
    assert_eq!(history.files.len(), 2);
    assert_eq!(history.couplings.len(), 1);
    assert_eq!(history.couplings[0].co_changes, 3);
    assert!((history.couplings[0].coupling_score - 1.0).abs() < f64::EPSILON);
    assert!(is_trackable_file("src/lib.rs"));
    assert!(!is_trackable_file("node_modules/pkg/index.js"));
    assert!(!is_trackable_file("image.png"));
}

#[test]
fn parsers_preserve_rename_destination_and_new_side_hunks() {
    let changes = parse_name_status("M\tsrc/a.rs\nR100\tsrc/old.rs\tsrc/new.rs\n", 10);
    assert_eq!(changes[1].path, "src/new.rs");
    assert_eq!(changes[1].old_path.as_deref(), Some("src/old.rs"));
    let hunks = parse_hunks("+++ b/src/a.rs\n@@ -1 +10,0 @@\n", 10);
    assert_eq!((hunks[0].start_line, hunks[0].end_line), (10, 10));
    assert_eq!(parse_range("2147483647,2"), (2_147_483_647, 2));
}

#[test]
fn context_and_changes_cover_dirty_untracked_rename_and_no_git() {
    let repo = repository();
    let context =
        resolve_context(repo.path(), &NeverCancel, &GitLimits::default()).expect("git context");
    assert!(context.is_git);
    assert_eq!(context.branch, "main");
    assert_eq!(
        context.branch_qualified_name("demo"),
        "demo.__branch__.main"
    );

    fs::write(repo.path().join("base.rs"), "fn changed() {}\n").expect("dirty");
    fs::write(repo.path().join("new.rs"), "fn new() {}\n").expect("untracked");
    git(repo.path(), &["mv", "base.rs", "renamed.rs"]);
    let changes = detect_changes(
        repo.path(),
        &DetectChangesOptions::default(),
        &NeverCancel,
        &GitLimits::default(),
    )
    .expect("changes");
    assert!(changes.files.contains(&"new.rs".to_owned()));
    assert!(changes.files.contains(&"renamed.rs".to_owned()));

    let plain = tempfile::tempdir().expect("plain directory");
    let context = resolve_context(plain.path(), &NeverCancel, &GitLimits::default())
        .expect("non-git context");
    assert!(!context.is_git);
    let changes = detect_changes(
        plain.path(),
        &DetectChangesOptions::default(),
        &NeverCancel,
        &GitLimits::default(),
    )
    .expect("non-git changes");
    assert!(changes.files.is_empty());
    assert!(changes.failure.is_some());
}

#[test]
fn history_uses_real_git_and_cancellation_is_observed_before_spawn() {
    let repo = repository();
    let history =
        collect_history(repo.path(), &NeverCancel, &GitLimits::default()).expect("history");
    assert_eq!(history.files[0].path, "base.rs");
    let cancelled = || true;
    let error =
        resolve_context(repo.path(), &cancelled, &GitLimits::default()).expect_err("cancelled");
    assert!(matches!(error, GitError::Cancelled));
}

#[test]
fn option_like_and_shell_metacharacter_references_are_rejected() {
    let repo = repository();
    for reference in ["--output=/tmp/pwn", "main;touch pwn"] {
        let error = detect_changes(
            repo.path(),
            &DetectChangesOptions {
                base_branch: reference.to_owned(),
                since: None,
            },
            &NeverCancel,
            &GitLimits::default(),
        )
        .expect_err("invalid reference");
        assert!(matches!(error, GitError::InvalidReference));
    }
}

#[test]
fn detached_linked_worktree_and_shallow_contexts_are_stable() {
    let temp = tempfile::tempdir().expect("temp");
    let main = temp.path().join("main repo");
    fs::create_dir(&main).expect("main repo");
    git(&main, &["init", "-q", "-b", "main"]);
    for revision in 0..3 {
        fs::write(
            main.join("lib.rs"),
            format!("fn value() {{ /* {revision} */ }}\n"),
        )
        .expect("source");
        git(&main, &["add", "lib.rs"]);
        git(
            &main,
            &["commit", "-q", "-m", &format!("revision {revision}")],
        );
    }

    let linked = temp.path().join("linked worktree");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature/worktree",
            linked.to_str().expect("linked path"),
        ],
    );
    let main_context =
        resolve_context(&main, &NeverCancel, &GitLimits::default()).expect("main context");
    let linked_context =
        resolve_context(&linked, &NeverCancel, &GitLimits::default()).expect("linked context");
    assert!(linked_context.is_worktree);
    assert_eq!(linked_context.branch_slug, "feature-worktree");
    assert_eq!(linked_context.canonical_root, main_context.canonical_root);

    git(&linked, &["checkout", "--detach", "-q"]);
    let detached =
        resolve_context(&linked, &NeverCancel, &GitLimits::default()).expect("detached context");
    assert!(detached.is_detached);
    assert_eq!(detached.branch, "DETACHED");
    assert_eq!(detached.branch_slug, "detached");

    let shallow = temp.path().join("shallow clone");
    let status = Command::new("git")
        .args(["clone", "-q", "--no-local", "--depth=1"])
        .arg(&main)
        .arg(&shallow)
        .status()
        .expect("shallow clone");
    assert!(status.success());
    assert!(shallow.join(".git/shallow").exists());
    let shallow_context =
        resolve_context(&shallow, &NeverCancel, &GitLimits::default()).expect("shallow context");
    assert!(shallow_context.is_git);
    assert!(!shallow_context.head_sha.is_empty());
    let changes = detect_changes(
        &shallow,
        &DetectChangesOptions {
            base_branch: "no-such-branch".to_owned(),
            since: Some("HEAD".to_owned()),
        },
        &NeverCancel,
        &GitLimits::default(),
    )
    .expect("shallow HEAD diff");
    assert!(changes.failure.is_none());
}
