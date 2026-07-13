use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ignore::{Match, WalkBuilder};

use crate::{DiscoveryError, DiscoveryOptions};

const MAX_IGNORE_WARNINGS: usize = 100;

#[derive(Debug)]
struct ScopedMatcher {
    root: PathBuf,
    matcher: Gitignore,
}

#[derive(Debug, Default)]
struct DirectoryRules {
    git: Option<ScopedMatcher>,
    cbm: Option<ScopedMatcher>,
}

#[derive(Debug)]
pub struct IgnoreRules {
    root: PathBuf,
    options: DiscoveryOptions,
    project: Vec<ScopedMatcher>,
    global: Option<ScopedMatcher>,
    directories: RefCell<BTreeMap<PathBuf, DirectoryRules>>,
    warnings: RefCell<Vec<String>>,
    warning_count: Cell<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuleMatch {
    None,
    Ignore,
    Whitelist,
}

impl IgnoreRules {
    /// Builds repository-level ignore rules. Directory-local rules are loaded
    /// only when their directory is visited or queried.
    ///
    /// # Errors
    ///
    /// Returns an error when the root or configured global ignore cannot be
    /// read, or when the configured global ignore contains an invalid pattern.
    pub fn build(root: &Path, options: &DiscoveryOptions) -> Result<Self, DiscoveryError> {
        let root = fs::canonicalize(root).map_err(|source| DiscoveryError::InvalidRoot {
            path: root.to_path_buf(),
            source,
        })?;
        if !root.is_dir() {
            return Err(DiscoveryError::NonDirectoryRoot { path: root });
        }

        let global = build_global_matcher(&root, options)?;
        let mut project = Vec::new();
        let mut warnings = Vec::new();
        for path in [
            root.join(".gitignore"),
            root.join(".git").join("info").join("exclude"),
        ] {
            match optional_matcher(&root, &path) {
                Ok(Some(matcher)) => project.push(matcher),
                Ok(None) => {}
                Err(warning) => warnings.push(warning),
            }
        }
        warnings.truncate(MAX_IGNORE_WARNINGS);

        let rules = Self {
            root,
            options: options.clone(),
            project,
            global,
            directories: RefCell::new(BTreeMap::new()),
            warning_count: Cell::new(warnings.len()),
            warnings: RefCell::new(warnings),
        };
        rules.load_directory(Path::new(""));
        Ok(rules)
    }

    #[must_use]
    pub fn is_explicitly_whitelisted(&self, path: &Path, is_dir: bool) -> bool {
        let absolute = self.absolute(path);
        self.ensure_parent_directories(path);
        self.cbm_match(&absolute, is_dir) == RuleMatch::Whitelist
    }

    #[must_use]
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let absolute = self.absolute(path);
        self.ensure_parent_directories(path);

        if self
            .project
            .iter()
            .any(|matcher| matched(matcher, &absolute, is_dir) == RuleMatch::Ignore)
        {
            return true;
        }
        if self
            .directories
            .borrow()
            .values()
            .filter_map(|rules| rules.git.as_ref())
            .any(|matcher| matched(matcher, &absolute, is_dir) == RuleMatch::Ignore)
        {
            return true;
        }

        let global_ignored = self
            .global
            .as_ref()
            .is_some_and(|matcher| matched(matcher, &absolute, is_dir) == RuleMatch::Ignore);
        match self.cbm_match(&absolute, is_dir) {
            RuleMatch::Ignore => true,
            RuleMatch::Whitelist => false,
            RuleMatch::None => global_ignored,
        }
    }

    /// Creates an `ignore` walker for callers that need its entry stream.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured external ignore cannot be loaded.
    pub fn walk_builder(&self) -> Result<WalkBuilder, DiscoveryError> {
        configured_walk_builder(&self.root, &self.options)
    }

    pub(crate) fn prepare_directory(&self, relative: &Path) {
        self.load_directory(relative);
    }

    pub(crate) fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut *self.warnings.borrow_mut())
    }

    fn absolute(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    fn ensure_parent_directories(&self, path: &Path) {
        let absolute = self.absolute(path);
        let relative = absolute.strip_prefix(&self.root).unwrap_or(path);
        let parent = relative.parent().unwrap_or_else(|| Path::new(""));
        self.load_directory(Path::new(""));

        let mut current = PathBuf::new();
        for component in parent.components() {
            if let Component::Normal(name) = component {
                current.push(name);
                self.load_directory(&current);
            }
        }
    }

    fn load_directory(&self, relative: &Path) {
        if self.directories.borrow().contains_key(relative) {
            return;
        }

        let directory = self.root.join(relative);
        let mut warnings = Vec::new();
        let git = if relative.as_os_str().is_empty() {
            None
        } else {
            load_directory_matcher(&directory.join(".gitignore"), &mut warnings)
        };
        let cbm = load_directory_matcher(&directory.join(".cbmignore"), &mut warnings);
        self.directories
            .borrow_mut()
            .insert(relative.to_path_buf(), DirectoryRules { git, cbm });
        for warning in warnings {
            self.push_warning(warning);
        }
    }

    fn cbm_match(&self, absolute: &Path, is_dir: bool) -> RuleMatch {
        let mut result = RuleMatch::None;
        for matcher in self
            .directories
            .borrow()
            .values()
            .filter_map(|rules| rules.cbm.as_ref())
        {
            match matched(matcher, absolute, is_dir) {
                RuleMatch::None => {}
                other => result = other,
            }
        }
        result
    }

    fn push_warning(&self, warning: String) {
        if self.warning_count.get() >= MAX_IGNORE_WARNINGS {
            return;
        }
        self.warning_count.set(self.warning_count.get() + 1);
        self.warnings.borrow_mut().push(warning);
    }
}

fn configured_walk_builder(
    root: &Path,
    options: &DiscoveryOptions,
) -> Result<WalkBuilder, DiscoveryError> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .follow_links(false)
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

fn build_global_matcher(
    root: &Path,
    options: &DiscoveryOptions,
) -> Result<Option<ScopedMatcher>, DiscoveryError> {
    if let Some(path) = &options.global_ignore_path {
        return matcher_from_file_with_root(root, path).map(Some);
    }

    let builder = GitignoreBuilder::new(root);
    let (matcher, error) = builder.build_global();
    if let Some(source) = error {
        return Err(DiscoveryError::IgnoreRule {
            path: root.to_path_buf(),
            source,
        });
    }
    if matcher.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ScopedMatcher {
            root: root.to_path_buf(),
            matcher,
        }))
    }
}

fn optional_matcher(root: &Path, path: &Path) -> Result<Option<ScopedMatcher>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Ok(None);
    }
    matcher_from_file_with_root(root, path)
        .map(Some)
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn load_directory_matcher(path: &Path, warnings: &mut Vec<String>) -> Option<ScopedMatcher> {
    let root = path.parent().unwrap_or_else(|| Path::new(""));
    match optional_matcher(root, path) {
        Ok(matcher) => matcher,
        Err(warning) => {
            warnings.push(warning);
            None
        }
    }
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

fn matched(matcher: &ScopedMatcher, path: &Path, is_dir: bool) -> RuleMatch {
    if !path.starts_with(&matcher.root) {
        return RuleMatch::None;
    }
    match matcher.matcher.matched_path_or_any_parents(path, is_dir) {
        Match::None => RuleMatch::None,
        Match::Ignore(_) => RuleMatch::Ignore,
        Match::Whitelist(_) => RuleMatch::Whitelist,
    }
}
