use std::fs;
use std::path::{Path, PathBuf};

use goldeneye_discovery::{
    DiscoveryError, DiscoveryOptions, DiscoveryReport, IgnoreReason, IndexMode,
    MAX_IGNORED_DETAILS, discover,
};
use tempfile::TempDir;

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

fn discovered_paths(report: &DiscoveryReport) -> Vec<PathBuf> {
    report
        .files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect()
}

fn has_ignored(report: &DiscoveryReport, path: &str, reason: IgnoreReason) -> bool {
    report
        .ignored
        .iter()
        .any(|ignored| ignored.relative_path == Path::new(path) && ignored.reason == reason)
}

#[test]
fn discovery_returns_supported_files_sorted_by_relative_path() {
    let repo = fixture(&[
        ("z.rs", "fn z() {}"),
        ("a.py", "def a(): pass"),
        ("notes.unknown", "ignored"),
        (".env", "A=1"),
    ]);

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert_eq!(
        discovered_paths(&report),
        [
            PathBuf::from(".env"),
            PathBuf::from("a.py"),
            PathBuf::from("z.rs")
        ]
    );
    assert!(has_ignored(
        &report,
        "notes.unknown",
        IgnoreReason::UnsupportedLanguage
    ));
}

#[test]
fn discovery_canonicalizes_root_and_file_paths() {
    let repo = fixture(&[("src/main.rs", "fn main() {}")]);
    let requested_root = repo.path().join(".");

    let report = discover(&requested_root, &DiscoveryOptions::default()).unwrap();

    assert_eq!(report.root, fs::canonicalize(repo.path()).unwrap());
    assert_eq!(report.files.len(), 1);
    assert_eq!(
        report.files[0].absolute_path,
        fs::canonicalize(repo.path().join("src/main.rs")).unwrap()
    );
    assert_eq!(report.files[0].relative_path, Path::new("src/main.rs"));
}

#[test]
fn discovery_rejects_missing_root() {
    let repo = tempfile::tempdir().expect("create parent");
    let missing = repo.path().join("missing");

    let error = discover(&missing, &DiscoveryOptions::default()).unwrap_err();

    assert!(matches!(error, DiscoveryError::InvalidRoot { path, .. } if path == missing));
}

#[test]
fn discovery_rejects_file_root() {
    let repo = fixture(&[("root.rs", "fn root() {}")]);
    let root_file = repo.path().join("root.rs");
    let canonical = fs::canonicalize(&root_file).unwrap();

    let error = discover(&root_file, &DiscoveryOptions::default()).unwrap_err();

    assert!(matches!(error, DiscoveryError::NonDirectoryRoot { path } if path == canonical));
}

#[test]
fn discovery_preserves_unicode_spaces_and_empty_files() {
    let repo = fixture(&[("目录/hello world.rs", "")]);

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert_eq!(
        discovered_paths(&report),
        [PathBuf::from("目录").join("hello world.rs")]
    );
    assert_eq!(report.files[0].byte_len, 0);
}

#[cfg(unix)]
#[test]
fn discovery_preserves_non_utf8_paths_without_lossy_conversion() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let repo = tempfile::tempdir().expect("create fixture root");
    let filename = OsString::from_vec(vec![b'n', 0x80, b'.', b'r', b's']);
    fs::write(repo.path().join(&filename), "fn native() {}").expect("write non-UTF-8 path");

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert_eq!(report.files.len(), 1);
    assert_eq!(report.files[0].relative_path, PathBuf::from(filename));
}

#[test]
fn discovery_applies_inclusive_size_cap_from_metadata() {
    let repo = fixture(&[("exact.rs", "12345"), ("large.rs", "123456")]);
    let options = DiscoveryOptions {
        max_file_bytes: 5,
        ..DiscoveryOptions::default()
    };

    let report = discover(repo.path(), &options).unwrap();

    assert_eq!(discovered_paths(&report), [PathBuf::from("exact.rs")]);
    assert_eq!(report.files[0].byte_len, 5);
    assert!(has_ignored(&report, "large.rs", IgnoreReason::Oversized));
}

#[test]
fn discovery_skips_file_symlinks_by_default_when_platform_allows_creation() {
    let repo = fixture(&[("small.rs", "fn x() {}")]);
    let link = repo.path().join("link.rs");
    if !try_create_file_symlink(&repo.path().join("small.rs"), &link) {
        return;
    }

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert_eq!(discovered_paths(&report), [PathBuf::from("small.rs")]);
    assert!(has_ignored(&report, "link.rs", IgnoreReason::Symlink));
}

#[test]
fn discovery_never_follows_outside_root_file_or_directory_links() {
    let repo = fixture(&[]);
    let outside = fixture(&[
        ("outside.rs", "fn outside() {}"),
        ("nested/inside.rs", "fn inside() {}"),
    ]);
    let file_link = repo.path().join("file-link.rs");
    let directory_link = repo.path().join("directory-link");
    let file_link_created = try_create_file_symlink(&outside.path().join("outside.rs"), &file_link);
    let directory_link_created = try_create_directory_link(outside.path(), &directory_link);
    if !file_link_created && !directory_link_created {
        return;
    }
    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(report.files.is_empty());
    if file_link_created {
        assert!(has_ignored(&report, "file-link.rs", IgnoreReason::Symlink));
    }
    if directory_link_created {
        assert!(has_ignored(
            &report,
            "directory-link",
            IgnoreReason::Symlink
        ));
    }
}

#[cfg(unix)]
fn try_create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("create file symlink");
    true
}

#[cfg(unix)]
fn try_create_directory_link(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("create directory symlink");
    true
}

#[cfg(windows)]
fn try_create_directory_link(target: &Path, link: &Path) -> bool {
    let output = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .expect("run mklink for directory junction");
    if output.status.success() {
        true
    } else if String::from_utf8_lossy(&output.stderr).contains("privilege")
        || String::from_utf8_lossy(&output.stdout).contains("privilege")
    {
        false
    } else {
        panic!(
            "create directory junction: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(windows)]
fn try_create_file_symlink(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => true,
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            false
        }
        Err(error) => panic!("create file symlink: {error}"),
    }
}

#[test]
fn cbm_whitelist_recovers_builtin_skipped_directory_before_policy() {
    let repo = fixture(&[
        (".cbmignore", "!vendor/\n!vendor/keep.rs\n"),
        ("vendor/keep.rs", "fn keep() {}"),
    ]);

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(
        discovered_paths(&report).contains(&PathBuf::from("vendor/keep.rs")),
        "explicit whitelist must bypass the vendor directory policy"
    );
    assert!(
        !report
            .excluded_directories
            .contains(&PathBuf::from("vendor"))
    );
}

#[test]
fn cbmignore_cannot_unskip_safety_core_directories() {
    for directory in [".git", "node_modules", ".worktrees", ".claude-worktrees"] {
        let keep = format!("{directory}/keep.rs");
        let cbmignore = format!("!{directory}/\n!{directory}/keep.rs\n");
        let repo = fixture(&[(".cbmignore", &cbmignore), (&keep, "fn keep() {}")]);

        let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

        assert!(
            !discovered_paths(&report)
                .iter()
                .any(|path| path.starts_with(directory)),
            "safety-core directory was traversed: {directory}"
        );
        assert!(
            report
                .excluded_directories
                .contains(&PathBuf::from(directory))
        );
    }
}

#[test]
fn cbmignore_cannot_resurrect_file_policy_matches() {
    let repo = fixture(&[
        (
            ".cbmignore",
            "!image.png\n!archive.zip\n!Cargo.lock\n!client.generated.rs\n",
        ),
        ("image.png", "png"),
        ("archive.zip", "zip"),
        ("Cargo.lock", "lock"),
        ("client.generated.rs", "fn generated() {}"),
    ]);

    let full = discover(repo.path(), &DiscoveryOptions::default()).unwrap();
    assert!(has_ignored(&full, "image.png", IgnoreReason::SuffixPolicy));

    let fast = discover(
        repo.path(),
        &DiscoveryOptions {
            mode: IndexMode::Fast,
            ..DiscoveryOptions::default()
        },
    )
    .unwrap();
    assert!(has_ignored(
        &fast,
        "archive.zip",
        IgnoreReason::SuffixPolicy
    ));
    assert!(has_ignored(
        &fast,
        "Cargo.lock",
        IgnoreReason::FilenamePolicy
    ));
    assert!(has_ignored(
        &fast,
        "client.generated.rs",
        IgnoreReason::PatternPolicy
    ));
}

#[test]
fn excluded_trees_do_not_load_nested_ignore_files() {
    let repo = fixture(&[("main.rs", "fn main() {}")]);
    let deep = repo.path().join("node_modules").join(
        (0..24)
            .map(|index| format!("d{index}"))
            .collect::<PathBuf>(),
    );
    fs::create_dir_all(&deep).expect("create excluded deep tree");
    fs::write(deep.join(".cbmignore"), "[\n").expect("write invalid nested ignore file");
    fs::create_dir_all(repo.path().join(".git/deep")).expect("create excluded git tree");
    fs::write(repo.path().join(".git/deep/.cbmignore"), "[\n")
        .expect("write invalid git ignore file");

    let report = discover(repo.path(), &DiscoveryOptions::default())
        .expect("excluded trees must not be scanned for ignore files");

    assert_eq!(discovered_paths(&report), [PathBuf::from("main.rs")]);
    assert!(report.warnings.is_empty());
}

#[test]
fn ignore_rules_exclude_files_before_language_classification() {
    let repo = fixture(&[
        (".gitignore", "ignored.rs\n"),
        ("ignored.rs", "fn ignored() {}"),
    ]);

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(report.files.is_empty());
    assert!(has_ignored(&report, "ignored.rs", IgnoreReason::IgnoreRule));
}

#[test]
fn moderate_and_fast_modes_exclude_generated_directories() {
    let repo = fixture(&[("generated/keep.rs", "fn keep() {}")]);

    let full = discover(
        repo.path(),
        &DiscoveryOptions {
            mode: IndexMode::Full,
            ..DiscoveryOptions::default()
        },
    )
    .unwrap();
    assert!(discovered_paths(&full).contains(&PathBuf::from("generated/keep.rs")));

    for mode in [IndexMode::Moderate, IndexMode::Fast] {
        let report = discover(
            repo.path(),
            &DiscoveryOptions {
                mode,
                ..DiscoveryOptions::default()
            },
        )
        .unwrap();
        assert!(report.files.is_empty());
        assert_eq!(report.excluded_directories, [PathBuf::from("generated")]);
        assert!(has_ignored(
            &report,
            "generated",
            IgnoreReason::DirectoryPolicy
        ));
    }
}

#[test]
fn full_mode_keeps_supported_generated_filename_while_fast_modes_filter_it() {
    let repo = fixture(&[("client.generated.rs", "fn generated() {}")]);

    let full = discover(repo.path(), &DiscoveryOptions::default()).unwrap();
    assert_eq!(
        discovered_paths(&full),
        [PathBuf::from("client.generated.rs")]
    );

    for mode in [IndexMode::Moderate, IndexMode::Fast] {
        let report = discover(
            repo.path(),
            &DiscoveryOptions {
                mode,
                ..DiscoveryOptions::default()
            },
        )
        .unwrap();
        assert!(has_ignored(
            &report,
            "client.generated.rs",
            IgnoreReason::PatternPolicy
        ));
    }
}

#[test]
fn exact_filename_policy_runs_before_unsupported_language_filter() {
    let repo = fixture(&[("Cargo.lock", "version = 3")]);
    let options = DiscoveryOptions {
        mode: IndexMode::Fast,
        ..DiscoveryOptions::default()
    };

    let report = discover(repo.path(), &options).unwrap();

    assert!(has_ignored(
        &report,
        "Cargo.lock",
        IgnoreReason::FilenamePolicy
    ));
}

#[test]
fn suffix_policy_runs_before_unsupported_language_filter() {
    let repo = fixture(&[("image.png", "not decoded")]);

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert!(has_ignored(
        &report,
        "image.png",
        IgnoreReason::SuffixPolicy
    ));
}

#[test]
fn ignored_details_are_sorted_bounded_and_keep_exact_total() {
    let repo = tempfile::tempdir().expect("create fixture root");
    let ignored_count = MAX_IGNORED_DETAILS + 7;
    for index in (0..ignored_count).rev() {
        fs::write(
            repo.path().join(format!("ignored-{index:04}.unknown")),
            "unsupported",
        )
        .expect("write ignored fixture");
    }

    let report = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert_eq!(report.ignored_total, ignored_count);
    assert_eq!(report.ignored.len(), MAX_IGNORED_DETAILS);
    assert_eq!(
        report.ignored.first().unwrap().relative_path,
        Path::new("ignored-0000.unknown")
    );
    assert_eq!(
        report.ignored.last().unwrap().relative_path,
        Path::new("ignored-0499.unknown")
    );
}

#[test]
fn report_collections_have_deterministic_path_order() {
    let repo = fixture(&[
        ("z-ignored.unknown", "x"),
        ("a-ignored.unknown", "x"),
        ("z.rs", "fn z() {}"),
        ("a.rs", "fn a() {}"),
        ("vendor/x.rs", "fn x() {}"),
        ("target/x.rs", "fn x() {}"),
    ]);

    let first = discover(repo.path(), &DiscoveryOptions::default()).unwrap();
    let second = discover(repo.path(), &DiscoveryOptions::default()).unwrap();

    assert_eq!(first, second);
    assert_eq!(
        discovered_paths(&first),
        [PathBuf::from("a.rs"), PathBuf::from("z.rs")]
    );
    assert_eq!(
        first.excluded_directories,
        [PathBuf::from("target"), PathBuf::from("vendor")]
    );
    let ignored_paths: Vec<_> = first
        .ignored
        .iter()
        .map(|ignored| ignored.relative_path.clone())
        .collect();
    assert_eq!(
        ignored_paths,
        [
            PathBuf::from("a-ignored.unknown"),
            PathBuf::from("target"),
            PathBuf::from("vendor"),
            PathBuf::from("z-ignored.unknown"),
        ]
    );
}

#[cfg(unix)]
#[test]
fn unreadable_directory_is_reported_when_permissions_are_enforced() {
    use std::os::unix::fs::PermissionsExt;

    let repo = fixture(&[("locked/inside.rs", "fn inside() {}")]);
    let locked = repo.path().join("locked");
    let original = fs::metadata(&locked).unwrap().permissions();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0)).unwrap();
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, original).unwrap();
        return;
    }

    let result = discover(repo.path(), &DiscoveryOptions::default());
    fs::set_permissions(&locked, original).unwrap();
    let report = result.expect("per-entry I/O failure must not abort discovery");

    assert!(has_ignored(&report, "locked", IgnoreReason::Io));
    assert!(!report.warnings.is_empty());
}
