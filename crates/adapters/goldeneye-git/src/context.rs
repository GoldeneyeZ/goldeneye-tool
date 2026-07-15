use std::path::{Path, PathBuf};

use goldeneye_ports::GitContext;

use super::{Cancellation, GitError, GitLimits, process::capture_one};

/// Resolves repository, worktree, branch, and revision metadata for a path.
///
/// # Errors
///
/// Returns a Git execution error when discovery is cancelled, times out, exceeds
/// configured output limits, or cannot start/read Git.
pub fn resolve_context(
    path: &Path,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<GitContext, GitError> {
    let root_exists = path.exists();
    let mut context = empty_context(path, root_exists);
    if !root_exists {
        return Ok(context);
    }
    let Some(worktree_root) = capture_one(
        path,
        &["rev-parse", "--show-toplevel"],
        cancellation,
        limits,
    )?
    else {
        return Ok(context);
    };
    context.is_git = true;
    context.worktree_root = worktree_root;
    populate_identity(path, &mut context, cancellation, limits)?;
    populate_repository_root(path, &mut context, cancellation, limits)?;
    context.base_sha = capture_one(
        path,
        &["merge-base", "HEAD", "@{upstream}"],
        cancellation,
        limits,
    )?
    .unwrap_or_default();
    Ok(context)
}

fn populate_identity(
    path: &Path,
    context: &mut GitContext,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<(), GitError> {
    context.git_dir =
        capture_one(path, &["rev-parse", "--git-dir"], cancellation, limits)?.unwrap_or_default();
    context.git_common_dir = capture_one(
        path,
        &["rev-parse", "--git-common-dir"],
        cancellation,
        limits,
    )?
    .unwrap_or_default();
    context.head_sha = capture_one(
        path,
        &["rev-parse", "--verify", "HEAD"],
        cancellation,
        limits,
    )?
    .unwrap_or_default();
    populate_branch(path, context, cancellation, limits)?;
    context.is_worktree = !context.git_dir.is_empty()
        && !context.git_common_dir.is_empty()
        && normalize_git_path(&context.git_dir) != normalize_git_path(&context.git_common_dir);
    Ok(())
}

fn populate_branch(
    path: &Path,
    context: &mut GitContext,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<(), GitError> {
    context.branch = capture_one(
        path,
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        cancellation,
        limits,
    )?
    .unwrap_or_else(|| {
        context.is_detached = true;
        "DETACHED".to_owned()
    });
    Ok(())
}

fn populate_repository_root(
    path: &Path,
    context: &mut GitContext,
    cancellation: &dyn Cancellation,
    limits: &GitLimits,
) -> Result<(), GitError> {
    let absolute_common = capture_one(
        path,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        cancellation,
        limits,
    )?;
    context.canonical_root = canonical_repository_root(
        path,
        Path::new(&context.worktree_root),
        &context.git_common_dir,
        absolute_common.as_deref(),
    );
    context.branch_slug = slug_from_branch(&context.branch, context.is_detached);
    Ok(())
}

fn empty_context(path: &Path, root_exists: bool) -> GitContext {
    GitContext {
        input_path: path.to_string_lossy().into_owned(),
        is_git: false,
        is_worktree: false,
        is_detached: false,
        root_exists,
        worktree_root: String::new(),
        git_dir: String::new(),
        git_common_dir: String::new(),
        canonical_root: String::new(),
        branch: String::new(),
        branch_slug: String::new(),
        head_sha: String::new(),
        base_sha: String::new(),
    }
}

fn normalize_git_path(value: &str) -> String {
    value.replace('\\', "/").trim_end_matches('/').to_owned()
}

fn canonical_repository_root(
    input: &Path,
    worktree_root: &Path,
    common_dir: &str,
    absolute_common: Option<&str>,
) -> String {
    let common = absolute_common
        .filter(|value| Path::new(value).is_absolute())
        .map_or_else(
            || {
                let value = Path::new(common_dir);
                if value.is_absolute() {
                    value.to_path_buf()
                } else if common_dir.is_empty() {
                    worktree_root.to_path_buf()
                } else {
                    input.join(value)
                }
            },
            PathBuf::from,
        );
    let common = common.canonicalize().unwrap_or(common);
    let root = if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().unwrap_or(&common).to_path_buf()
    } else {
        common
    };
    root.to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_owned()
}

fn slug_from_branch(branch: &str, detached: bool) -> String {
    let fallback = if detached { "detached" } else { "working-tree" };
    let source = if detached || branch.is_empty() {
        fallback
    } else {
        branch
    };
    let mut slug = String::new();
    let mut in_dash = false;
    for character in source.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
            if slug.is_empty() && character == '-' {
                in_dash = true;
                continue;
            }
            slug.push(character);
            in_dash = false;
        } else if !slug.is_empty() && !in_dash {
            slug.push('-');
            in_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        fallback.to_owned()
    } else {
        slug
    }
}
