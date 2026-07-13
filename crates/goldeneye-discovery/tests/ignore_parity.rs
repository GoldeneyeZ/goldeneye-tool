use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::Path;

use goldeneye_discovery::{
    DiscoveryOptions, IgnoreReason, IgnoreRules, IndexMode, directory_policy, file_policy,
};
use tempfile::{NamedTempFile, TempDir};

fn fixture(files: &[(&str, &str)]) -> TempDir {
    let root = tempfile::tempdir().expect("create fixture root");
    for (relative, contents) in files {
        let path = root.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create fixture directories");
        }
        fs::write(path, contents).expect("write fixture file");
    }
    root
}

#[test]
fn nested_gitignore_stacks_with_root() {
    let repo = fixture(&[
        (".gitignore", "root.log\n"),
        ("src/.gitignore", "generated/\n"),
        ("root.log", "x"),
        ("src/generated/x.rs", "fn x() {}"),
        ("src/main.rs", "fn main() {}"),
    ]);

    let rules = IgnoreRules::build(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(rules.is_ignored(Path::new("root.log"), false));
    assert!(rules.is_ignored(Path::new("src/generated"), true));
    assert!(!rules.is_ignored(Path::new("src/main.rs"), false));
}

#[test]
fn cbmignore_negates_global_and_builtin_skips() {
    let repo = fixture(&[
        (".cbmignore", "!vendor/\n!vendor/keep.rs\n"),
        ("vendor/keep.rs", "fn keep() {}"),
    ]);
    let mut global = NamedTempFile::new().expect("create external ignore");
    global
        .write_all(b"vendor/\n")
        .expect("write external ignore");
    let options = DiscoveryOptions {
        global_ignore_path: Some(global.path().to_path_buf()),
        ..DiscoveryOptions::default()
    };

    let rules = IgnoreRules::build(repo.path(), &options).unwrap();

    assert!(rules.is_explicitly_whitelisted(Path::new("vendor"), true));
    assert!(!rules.is_ignored(Path::new("vendor/keep.rs"), false));
    let walked: Vec<_> = rules
        .walk_builder()
        .unwrap()
        .build()
        .filter_map(Result::ok)
        .map(ignore::DirEntry::into_path)
        .collect();
    assert!(
        walked.contains(
            &fs::canonicalize(repo.path().join("vendor/keep.rs"))
                .expect("canonicalize expected walked path")
        )
    );
    assert_eq!(
        directory_policy(OsStr::new("vendor"), IndexMode::Full),
        Some(IgnoreReason::DirectoryPolicy)
    );
}

#[test]
fn gitignore_syntax_comments_escapes_roots_directories_and_globstars() {
    let repo = fixture(&[
        (
            ".cbmignore",
            "# comment\n\\#literal\n\\!important\n/root-only.txt\ncache/\n**/generated/*.rs\n",
        ),
        ("#literal", "x"),
        ("!important", "x"),
        ("root-only.txt", "x"),
        ("nested/root-only.txt", "x"),
        ("cache/x.rs", "x"),
        ("cache.txt", "x"),
        ("deep/generated/x.rs", "x"),
    ]);

    let rules = IgnoreRules::build(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(rules.is_ignored(Path::new("#literal"), false));
    assert!(rules.is_ignored(Path::new("!important"), false));
    assert!(rules.is_ignored(Path::new("root-only.txt"), false));
    assert!(!rules.is_ignored(Path::new("nested/root-only.txt"), false));
    assert!(rules.is_ignored(Path::new("cache"), true));
    assert!(!rules.is_ignored(Path::new("cache.txt"), false));
    assert!(rules.is_ignored(Path::new("deep/generated/x.rs"), false));
}

#[test]
fn custom_rules_are_last_match_wins_and_nested_rules_have_highest_precedence() {
    let repo = fixture(&[
        (".cbmignore", "*.rs\n!root_keep.rs\n"),
        ("root_keep.rs", "x"),
        ("root_drop.rs", "x"),
        ("src/.cbmignore", "!keep.rs\ndrop.rs\n"),
        ("src/keep.rs", "x"),
        ("src/drop.rs", "x"),
    ]);

    let rules = IgnoreRules::build(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(rules.is_explicitly_whitelisted(Path::new("root_keep.rs"), false));
    assert!(!rules.is_ignored(Path::new("root_keep.rs"), false));
    assert!(rules.is_ignored(Path::new("root_drop.rs"), false));
    assert!(rules.is_explicitly_whitelisted(Path::new("src/keep.rs"), false));
    assert!(!rules.is_ignored(Path::new("src/keep.rs"), false));
    assert!(rules.is_ignored(Path::new("src/drop.rs"), false));
}

#[test]
fn gitignore_is_honored_without_git_metadata() {
    let repo = fixture(&[(".gitignore", "ignored.rs\n"), ("ignored.rs", "x")]);
    assert!(!repo.path().join(".git").exists());

    let rules = IgnoreRules::build(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(rules.is_ignored(Path::new("ignored.rs"), false));
    let walked: Vec<_> = rules
        .walk_builder()
        .unwrap()
        .build()
        .filter_map(Result::ok)
        .map(ignore::DirEntry::into_path)
        .collect();
    assert!(!walked.contains(&repo.path().join("ignored.rs")));
}

#[test]
fn directory_policy_is_case_sensitive_and_mode_gated() {
    for name in [".git", "vendored"] {
        for mode in [IndexMode::Full, IndexMode::Moderate, IndexMode::Fast] {
            assert_eq!(
                directory_policy(OsStr::new(name), mode),
                Some(IgnoreReason::DirectoryPolicy)
            );
        }
    }
    for name in ["generated", "out"] {
        assert_eq!(directory_policy(OsStr::new(name), IndexMode::Full), None);
        assert_eq!(
            directory_policy(OsStr::new(name), IndexMode::Moderate),
            Some(IgnoreReason::DirectoryPolicy)
        );
        assert_eq!(
            directory_policy(OsStr::new(name), IndexMode::Fast),
            Some(IgnoreReason::DirectoryPolicy)
        );
    }
    assert_eq!(
        directory_policy(OsStr::new("Vendor"), IndexMode::Fast),
        None
    );
}

#[test]
fn file_policy_applies_suffix_filename_pattern_and_json_tables() {
    for mode in [IndexMode::Full, IndexMode::Moderate, IndexMode::Fast] {
        assert_eq!(
            file_policy(OsStr::new("image.png"), mode),
            Some(IgnoreReason::SuffixPolicy)
        );
        assert_eq!(
            file_policy(OsStr::new("package.json"), mode),
            Some(IgnoreReason::FilenamePolicy)
        );
    }

    for (name, reason) in [
        ("archive.zip", IgnoreReason::SuffixPolicy),
        ("LICENSE", IgnoreReason::FilenamePolicy),
        ("client.generated.rs", IgnoreReason::PatternPolicy),
    ] {
        assert_eq!(file_policy(OsStr::new(name), IndexMode::Full), None);
        assert_eq!(
            file_policy(OsStr::new(name), IndexMode::Moderate),
            Some(reason)
        );
        assert_eq!(file_policy(OsStr::new(name), IndexMode::Fast), Some(reason));
    }

    assert_eq!(file_policy(OsStr::new("IMAGE.PNG"), IndexMode::Fast), None);
    assert_eq!(
        file_policy(OsStr::new("ordinary.rs"), IndexMode::Fast),
        None
    );
}
