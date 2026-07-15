use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use goldeneye_edit::path_auth::{PathAuthorizationError, PathAuthorizer, PathIntent};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "goldeneye-edit-path-auth-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create path authorization fixture");
        Self { root }
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(&path).expect("create fixture directory");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn authorizer(allowed_root: &Path) -> PathAuthorizer {
    PathAuthorizer::new([allowed_root]).expect("configure allowed root")
}

#[test]
fn rejects_non_normalized_absolute_traversal_and_prefixed_paths() {
    let fixture = Fixture::new();
    let allowed = fixture.directory("allowed");
    let project = fixture.directory("allowed/project");
    let authorizer = authorizer(&allowed);

    for relative in [
        "",
        ".",
        "./src/lib.rs",
        "src//lib.rs",
        "src/../secret.rs",
        "../secret.rs",
        "/absolute.rs",
        "C:/prefixed.rs",
        r"src\backslash.rs",
    ] {
        let result = authorizer.authorize(&project, relative, PathIntent::Create);
        assert!(
            matches!(
                result,
                Err(PathAuthorizationError::InvalidRelativePath { .. })
            ),
            "unexpected authorization result for {relative:?}"
        );
    }
}

#[test]
fn rejects_reserved_tool_metadata_at_any_depth_and_case() {
    let fixture = Fixture::new();
    let allowed = fixture.directory("allowed");
    let project = fixture.directory("allowed/project");
    let authorizer = authorizer(&allowed);

    for relative in [
        ".goldeneye/state.json",
        "src/.codebase-memory/index.db",
        "nested/.GOLDENEYE/state.json",
    ] {
        let result = authorizer.authorize(&project, relative, PathIntent::Create);
        assert!(matches!(
            result,
            Err(PathAuthorizationError::ReservedMetadata { .. })
        ));
    }
}

#[test]
fn rejects_project_roots_outside_configured_allowed_roots() {
    let fixture = Fixture::new();
    let allowed = fixture.directory("allowed");
    let outside_project = fixture.directory("outside/project");

    let result = authorizer(&allowed).authorize(&outside_project, "src/lib.rs", PathIntent::Create);

    assert!(matches!(
        result,
        Err(PathAuthorizationError::ProjectOutsideAllowedRoots { .. })
    ));
}

#[test]
fn create_intent_rejects_an_existing_destination() {
    let fixture = Fixture::new();
    let allowed = fixture.directory("allowed");
    let project = fixture.directory("allowed/project");
    let canonical_project = fs::canonicalize(&project).expect("canonicalize project fixture");
    let destination = project.join("existing.rs");
    fs::write(&destination, "fn existing() {}\n").expect("write existing fixture");

    let result = authorizer(&allowed).authorize(&project, "existing.rs", PathIntent::Create);

    assert!(matches!(
        result,
        Err(PathAuthorizationError::DestinationExists { path })
            if path == canonical_project.join("existing.rs")
    ));
}

#[test]
fn accepts_normalized_unicode_paths() {
    let fixture = Fixture::new();
    let allowed = fixture.directory("allowed");
    let project = fixture.directory("allowed/project");
    let canonical_project = fs::canonicalize(&project).expect("canonicalize project fixture");
    fixture.directory("allowed/project/src");

    let authorized = authorizer(&allowed)
        .authorize(&project, "src/naïve_变量.rs", PathIntent::Create)
        .expect("authorize Unicode path");
    let revalidated = authorized.revalidate().expect("revalidate Unicode path");

    assert_eq!(
        revalidated.as_path(),
        canonical_project.join("src/naïve_变量.rs")
    );
}

#[test]
fn existing_ancestor_escape_is_rejected_and_revalidated_after_authorization() {
    let fixture = Fixture::new();
    let allowed = fixture.directory("allowed");
    let project = fixture.directory("allowed/project");
    let outside = fixture.directory("outside");
    let safe_parent = fixture.directory("allowed/project/pending");
    let guard = authorizer(&allowed);
    let authorized = guard
        .authorize(&project, "pending/file.rs", PathIntent::Create)
        .expect("initial safe authorization");

    fs::remove_dir(&safe_parent).expect("remove safe parent");
    if create_directory_link(&outside, &safe_parent).is_err() {
        return;
    }

    let result = authorized.revalidate();
    assert!(matches!(
        result,
        Err(PathAuthorizationError::PathEscapesProject { .. })
    ));

    remove_directory_link(&safe_parent).expect("remove directory link");
}

#[test]
fn parent_creation_is_confined_reported_and_rollback_removes_only_empty_directories() {
    let fixture = Fixture::new();
    let allowed = fixture.directory("allowed");
    let project = fixture.directory("allowed/project");
    let canonical_project = fs::canonicalize(&project).expect("canonicalize project fixture");
    let authorized = authorizer(&allowed)
        .authorize(&project, "nested/deep/file.rs", PathIntent::Create)
        .expect("authorize nested creation");

    let created = authorized
        .create_parent_directories()
        .expect("create confined parents");
    assert_eq!(
        created.paths(),
        &[
            canonical_project.join("nested"),
            canonical_project.join("nested/deep"),
        ]
    );
    authorized
        .revalidate()
        .expect("revalidate after parent creation");

    fs::write(project.join("nested/deep/keep.txt"), "keep")
        .expect("make deepest directory non-empty");
    created
        .rollback_empty()
        .expect("rollback skips non-empty directories");
    assert!(project.join("nested/deep").is_dir());

    fs::remove_file(project.join("nested/deep/keep.txt")).expect("remove retained fixture");
    created
        .rollback_empty()
        .expect("rollback empty created directories");
    assert!(!project.join("nested").exists());
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(unix)]
fn remove_directory_link(link: &Path) -> io::Result<()> {
    fs::remove_file(link)
}

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> io::Result<()> {
    use std::process::{Command, Stdio};

    let status = Command::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other("mklink /J failed"))
    }
}

#[cfg(windows)]
fn remove_directory_link(link: &Path) -> io::Result<()> {
    fs::remove_dir(link)
}
