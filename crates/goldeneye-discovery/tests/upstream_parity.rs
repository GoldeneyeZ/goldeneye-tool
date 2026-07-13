use std::fs;
use std::path::Path;

use goldeneye_discovery::{DiscoveryOptions, DiscoveryReport, IgnoreReason, IndexMode, discover};
use tempfile::TempDir;

const MANIFEST: &str = include_str!("fixtures/discovery/manifest.tsv");

#[derive(Debug, PartialEq, Eq)]
struct NormalizedFile {
    path: String,
    language_id: String,
    byte_len: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct NormalizedIgnored {
    path: String,
    reason: String,
}

#[derive(Debug, PartialEq, Eq)]
struct NormalizedReport {
    mode: &'static str,
    files: Vec<NormalizedFile>,
    excluded_directories: Vec<String>,
    ignored: Vec<NormalizedIgnored>,
    ignored_total: usize,
    warnings: Vec<String>,
}

struct UpstreamFixture {
    repository: TempDir,
    global_ignore: TempDir,
    symlink_available: bool,
}

impl UpstreamFixture {
    fn materialize() -> Self {
        let repository = tempfile::tempdir().expect("create fixture repository");
        for (path, contents) in [
            (".gitignore", "root-ignored.py\n"),
            (".cbmignore", "!rescued.go\n!obj/\nroot-cbmignored.py\n"),
            ("src/main.rs", "fn main() {}\n"),
            ("src/naïve file.py", "print('ok')\n"),
            ("notes.unknown", "ignored\n"),
            ("Dockerfile", "FROM scratch\n"),
            ("views/home.blade.php", "{{ value }}\n"),
            (".env.local", "A=1\n"),
            ("root-ignored.py", "ignored\n"),
            ("root-cbmignored.py", "ignored\n"),
            ("global-ignored.go", "package ignored\n"),
            ("rescued.go", "package rescued\n"),
            ("nested/.gitignore", "local-ignored.rs\nrescued-local.rs\n"),
            ("nested/.cbmignore", "!rescued-local.rs\n"),
            ("nested/local-ignored.rs", "fn ignored() {}\n"),
            ("nested/rescued-local.rs", "fn rescued() {}\n"),
            ("node_modules/dependency.js", "ignored();\n"),
            ("docs/guide.md", "# Guide\n"),
            ("obj/kept.go", "package kept\n"),
            ("compiled.pyc", "bytecode\n"),
            ("archive.zip", "archive\n"),
            ("LICENSE", "license\n"),
            ("app.bundle.js", "bundle();\n"),
            ("large.rs", &"x".repeat(128)),
        ] {
            write_fixture_file(repository.path(), path, contents);
        }

        let symlink_available = create_file_symlink(
            &repository.path().join("src/main.rs"),
            &repository.path().join("link.rs"),
        );

        let global_ignore = tempfile::tempdir().expect("create global-ignore fixture");
        fs::write(
            global_ignore.path().join("global.ignore"),
            "global-ignored.go\nrescued.go\n",
        )
        .expect("write global ignore");

        Self {
            repository,
            global_ignore,
            symlink_available,
        }
    }

    fn root(&self) -> &Path {
        self.repository.path()
    }

    fn options(&self, mode: IndexMode) -> DiscoveryOptions {
        DiscoveryOptions {
            mode,
            max_file_bytes: 64,
            global_ignore_path: Some(self.global_ignore.path().join("global.ignore")),
            ..DiscoveryOptions::default()
        }
    }

    fn expected(&self, mode: IndexMode) -> NormalizedReport {
        expected_report(mode, self.symlink_available)
    }
}

#[test]
fn full_moderate_and_fast_reports_match_frozen_upstream_manifest() {
    let fixture = UpstreamFixture::materialize();
    for mode in [IndexMode::Full, IndexMode::Moderate, IndexMode::Fast] {
        let actual = discover(fixture.root(), &fixture.options(mode)).unwrap();
        assert_eq!(actual.root, fs::canonicalize(fixture.root()).unwrap());
        assert_eq!(normalize_report(mode, actual), fixture.expected(mode));
    }
}

#[test]
fn normalization_omits_only_platform_permission_warnings() {
    let warnings = vec![
        "locked: Permission denied (os error 13)".to_owned(),
        "locked: Access is denied. (os error 5)".to_owned(),
        "kept: metadata read failed".to_owned(),
    ];

    assert_eq!(
        normalize_warnings(warnings),
        vec!["kept: metadata read failed"]
    );
}

fn normalize_report(mode: IndexMode, report: DiscoveryReport) -> NormalizedReport {
    NormalizedReport {
        mode: mode_name(mode),
        files: report
            .files
            .into_iter()
            .map(|file| NormalizedFile {
                path: normalize_path(&file.relative_path),
                language_id: file.language.as_str().to_owned(),
                byte_len: file.byte_len,
            })
            .collect(),
        excluded_directories: report
            .excluded_directories
            .iter()
            .map(|path| normalize_path(path))
            .collect(),
        ignored: report
            .ignored
            .into_iter()
            .map(|ignored| NormalizedIgnored {
                path: normalize_path(&ignored.relative_path),
                reason: reason_name(ignored.reason).to_owned(),
            })
            .collect(),
        ignored_total: report.ignored_total,
        warnings: normalize_warnings(report.warnings),
    }
}

fn expected_report(mode: IndexMode, symlink_available: bool) -> NormalizedReport {
    let mut files = Vec::new();
    let mut excluded_directories = Vec::new();
    let mut ignored = Vec::new();

    for (line_index, line) in MANIFEST.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 7, "manifest line {}", line_index + 1);
        if fields[1] != mode_name(mode) {
            continue;
        }
        if fields[0] == "link.rs" && !symlink_available {
            continue;
        }
        assert!(
            fields[6].contains("@2469ecc3"),
            "manifest line {} lacks pinned upstream citation",
            line_index + 1
        );

        match fields[2] {
            "file" => files.push(NormalizedFile {
                path: fields[0].to_owned(),
                language_id: fields[3].to_owned(),
                byte_len: fields[5].parse().expect("manifest file byte length"),
            }),
            "ignored" | "excluded_directory" => {
                assert!(fields[3].is_empty());
                assert!(fields[5].is_empty());
                if fields[2] == "excluded_directory" {
                    excluded_directories.push(fields[0].to_owned());
                }
                ignored.push(NormalizedIgnored {
                    path: fields[0].to_owned(),
                    reason: fields[4].to_owned(),
                });
            }
            disposition => panic!("unknown disposition {disposition}"),
        }
    }

    NormalizedReport {
        mode: mode_name(mode),
        ignored_total: ignored.len(),
        files,
        excluded_directories,
        ignored,
        warnings: Vec::new(),
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_str()
        .expect("fixture paths are valid UTF-8")
        .replace('\\', "/")
}

fn normalize_warnings(warnings: Vec<String>) -> Vec<String> {
    warnings
        .into_iter()
        .filter(|warning| {
            let warning = warning.to_ascii_lowercase();
            !warning.contains("permission denied") && !warning.contains("access is denied")
        })
        .collect()
}

const fn mode_name(mode: IndexMode) -> &'static str {
    match mode {
        IndexMode::Full => "full",
        IndexMode::Moderate => "moderate",
        IndexMode::Fast => "fast",
    }
}

const fn reason_name(reason: IgnoreReason) -> &'static str {
    match reason {
        IgnoreReason::IgnoreRule => "ignore_rule",
        IgnoreReason::DirectoryPolicy => "directory_policy",
        IgnoreReason::SuffixPolicy => "suffix_policy",
        IgnoreReason::FilenamePolicy => "filename_policy",
        IgnoreReason::PatternPolicy => "pattern_policy",
        IgnoreReason::Oversized => "oversized",
        IgnoreReason::UnsupportedLanguage => "unsupported_language",
        IgnoreReason::Symlink => "symlink",
        IgnoreReason::Io => "io",
    }
}

fn write_fixture_file(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(path, contents).expect("write fixture file");
}

#[cfg(unix)]
fn create_file_symlink(target: &Path, link: &Path) -> bool {
    std::os::unix::fs::symlink(target, link).expect("create fixture symlink");
    true
}

#[cfg(windows)]
fn create_file_symlink(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => true,
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || error.raw_os_error() == Some(1314) =>
        {
            false
        }
        Err(error) => panic!("create fixture symlink: {error}"),
    }
}
