use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::{Match, WalkBuilder};

use crate::{DiscoveryError, DiscoveryOptions};

#[derive(Debug)]
struct ScopedMatcher {
    root: PathBuf,
    matcher: Gitignore,
}

#[derive(Debug, Default)]
struct CbmIgnoreIndex {
    matchers: Vec<ScopedMatcher>,
}

#[derive(Debug)]
pub struct IgnoreRules {
    root: PathBuf,
    options: DiscoveryOptions,
    standard: Vec<ScopedMatcher>,
    cbm: CbmIgnoreIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleMatch {
    None,
    Ignore,
    Whitelist,
}

impl IgnoreRules {
    /// Builds Git-compatible repository ignore rules and a high-precedence
    /// `.cbmignore` index.
    ///
    /// # Errors
    ///
    /// Returns an error when the root or an ignore file cannot be read, or an
    /// ignore pattern is invalid.
    pub fn build(root: &Path, options: &DiscoveryOptions) -> Result<Self, DiscoveryError> {
        let root = fs::canonicalize(root).map_err(|source| DiscoveryError::InvalidRoot {
            path: root.to_path_buf(),
            source,
        })?;
        if !root.is_dir() {
            return Err(DiscoveryError::NonDirectoryRoot { path: root });
        }

        configured_walk_builder(&root, options)?;
        let standard = build_standard_index(&root, options)?;
        let cbm = CbmIgnoreIndex::build(&root)?;
        Ok(Self {
            root,
            options: options.clone(),
            standard,
            cbm,
        })
    }

    #[must_use]
    pub fn is_explicitly_whitelisted(&self, path: &Path, is_dir: bool) -> bool {
        self.cbm.matched(&self.absolute(path), is_dir) == RuleMatch::Whitelist
    }

    #[must_use]
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let absolute = self.absolute(path);
        match self.cbm.matched(&absolute, is_dir) {
            RuleMatch::Ignore => true,
            RuleMatch::Whitelist => false,
            RuleMatch::None => {
                effective_match(&self.standard, &absolute, is_dir) == RuleMatch::Ignore
            }
        }
    }

    /// Creates a walker with the same ignore sources and precedence as this
    /// rule set.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured external ignore cannot be loaded.
    pub fn walk_builder(&self) -> Result<WalkBuilder, DiscoveryError> {
        configured_walk_builder(&self.root, &self.options)
    }

    fn absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }
}

impl CbmIgnoreIndex {
    fn build(root: &Path) -> Result<Self, DiscoveryError> {
        let files = find_named_files(root, OsStr::new(".cbmignore"));
        let matchers = files
            .iter()
            .map(|path| matcher_from_file(path))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { matchers })
    }

    fn matched(&self, path: &Path, is_dir: bool) -> RuleMatch {
        effective_match(&self.matchers, path, is_dir)
    }
}

fn configured_walk_builder(
    root: &Path,
    options: &DiscoveryOptions,
) -> Result<WalkBuilder, DiscoveryError> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(options.follow_symlinks)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(options.global_ignore_path.is_none())
        .parents(true)
        .add_custom_ignore_filename(".cbmignore");
    if let Some(path) = &options.global_ignore_path
        && let Some(source) = builder.add_ignore(path)
    {
        return Err(DiscoveryError::IgnoreRule {
            path: path.clone(),
            source,
        });
    }
    Ok(builder)
}

fn build_standard_index(
    root: &Path,
    options: &DiscoveryOptions,
) -> Result<Vec<ScopedMatcher>, DiscoveryError> {
    let mut matchers = Vec::new();
    if let Some(path) = &options.global_ignore_path {
        matchers.push(matcher_from_file_with_root(root, path)?);
    } else {
        let builder = GitignoreBuilder::new(root);
        let (matcher, error) = builder.build_global();
        if let Some(source) = error {
            return Err(DiscoveryError::IgnoreRule {
                path: root.to_path_buf(),
                source,
            });
        }
        if !matcher.is_empty() {
            matchers.push(ScopedMatcher {
                root: root.to_path_buf(),
                matcher,
            });
        }
    }

    let exclude = root.join(".git").join("info").join("exclude");
    if exclude.is_file() {
        matchers.push(matcher_from_file_with_root(root, &exclude)?);
    }
    for path in find_named_files(root, OsStr::new(".gitignore")) {
        matchers.push(matcher_from_file(&path)?);
    }
    Ok(matchers)
}

fn matcher_from_file(path: &Path) -> Result<ScopedMatcher, DiscoveryError> {
    let root = path.parent().unwrap_or_else(|| Path::new("."));
    matcher_from_file_with_root(root, path)
}

fn matcher_from_file_with_root(root: &Path, path: &Path) -> Result<ScopedMatcher, DiscoveryError> {
    let mut builder = GitignoreBuilder::new(root);
    if let Some(source) = builder.add(path) {
        return Err(DiscoveryError::IgnoreRule {
            path: path.to_path_buf(),
            source,
        });
    }
    let matcher = builder
        .build()
        .map_err(|source| DiscoveryError::IgnoreRule {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(ScopedMatcher {
        root: root.to_path_buf(),
        matcher,
    })
}

fn effective_match(matchers: &[ScopedMatcher], path: &Path, is_dir: bool) -> RuleMatch {
    let mut result = RuleMatch::None;
    for scoped in matchers {
        if !path.starts_with(&scoped.root) {
            continue;
        }
        result = match scoped.matcher.matched_path_or_any_parents(path, is_dir) {
            Match::None => result,
            Match::Ignore(_) => RuleMatch::Ignore,
            Match::Whitelist(_) => RuleMatch::Whitelist,
        };
    }
    result
}

fn find_named_files(root: &Path, name: &OsStr) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_named_files(root, name, &mut found);
    found.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });
    found
}

fn collect_named_files(directory: &Path, name: &OsStr, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    let mut entries = entries.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect_named_files(&path, name, found);
        } else if metadata.is_file() && entry.file_name() == name {
            found.push(path);
        }
    }
}
