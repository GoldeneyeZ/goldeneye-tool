use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::{
    DiscoveredFile, DiscoveryError, DiscoveryOptions, DiscoveryReport, IgnoreReason, IgnoreRules,
    IgnoredPath, LanguageRegistry, directory_policy, file_policy,
};

pub const MAX_IGNORED_DETAILS: usize = 500;

/// Discovers supported source files below a repository root.
///
/// # Errors
///
/// Returns an error when the root is missing or not a directory, ignore rules
/// are invalid, or language overrides are invalid. Per-entry I/O failures are
/// retained in the report and do not abort the walk.
pub fn discover(
    root: &Path,
    options: &DiscoveryOptions,
) -> Result<DiscoveryReport, DiscoveryError> {
    let root = fs::canonicalize(root).map_err(|source| DiscoveryError::InvalidRoot {
        path: root.to_path_buf(),
        source,
    })?;
    if !root.is_dir() {
        return Err(DiscoveryError::NonDirectoryRoot { path: root });
    }

    let rules = IgnoreRules::build(&root, options)?;
    let languages = LanguageRegistry::with_overrides(options.extension_overrides.clone())?;
    let mut walker = RepositoryWalker::new(root, options, &rules, &languages);
    walker.walk();
    Ok(walker.finish())
}

struct RepositoryWalker<'a> {
    root: PathBuf,
    options: &'a DiscoveryOptions,
    rules: &'a IgnoreRules,
    languages: &'a LanguageRegistry,
    directories: Vec<(PathBuf, PathBuf)>,
    visited_directories: HashSet<PathBuf>,
    files: Vec<DiscoveredFile>,
    excluded_directories: Vec<PathBuf>,
    ignored: Vec<IgnoredPath>,
    ignored_total: usize,
    warnings: Vec<String>,
}

impl<'a> RepositoryWalker<'a> {
    fn new(
        root: PathBuf,
        options: &'a DiscoveryOptions,
        rules: &'a IgnoreRules,
        languages: &'a LanguageRegistry,
    ) -> Self {
        Self {
            directories: vec![(root.clone(), PathBuf::new())],
            root,
            options,
            rules,
            languages,
            visited_directories: HashSet::new(),
            files: Vec::new(),
            excluded_directories: Vec::new(),
            ignored: Vec::new(),
            ignored_total: 0,
            warnings: Vec::new(),
        }
    }

    fn walk(&mut self) {
        while let Some((absolute, relative)) = self.directories.pop() {
            if self.options.follow_symlinks {
                match fs::canonicalize(&absolute) {
                    Ok(canonical) => {
                        if !self.visited_directories.insert(canonical) {
                            continue;
                        }
                    }
                    Err(error) => {
                        self.record_io(&relative, &error);
                        continue;
                    }
                }
            }

            let entries = match fs::read_dir(&absolute) {
                Ok(entries) => entries,
                Err(error) => {
                    self.record_io(&relative, &error);
                    continue;
                }
            };
            for entry in entries {
                match entry {
                    Ok(entry) => self.process_entry(&entry, &relative),
                    Err(error) => self.record_io(&relative, &error),
                }
            }
        }
    }

    fn process_entry(&mut self, entry: &fs::DirEntry, parent_relative: &Path) {
        let relative = parent_relative.join(entry.file_name());
        let absolute = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                self.record_io(&relative, &error);
                return;
            }
        };

        if file_type.is_symlink() {
            if !self.options.follow_symlinks {
                self.record_ignored(relative, IgnoreReason::Symlink, None);
                return;
            }
            match fs::metadata(&absolute) {
                Ok(metadata) if metadata.is_dir() => self.process_directory(absolute, relative),
                Ok(metadata) if metadata.is_file() => {
                    self.process_file(&absolute, relative, Some(metadata));
                }
                Ok(_) => {}
                Err(error) => self.record_io(&relative, &error),
            }
        } else if file_type.is_dir() {
            self.process_directory(absolute, relative);
        } else if file_type.is_file() {
            self.process_file(&absolute, relative, None);
        }
    }

    fn process_directory(&mut self, absolute: PathBuf, relative: PathBuf) {
        let whitelisted = self.rules.is_explicitly_whitelisted(&relative, true);
        let reason = if self.rules.is_ignored(&relative, true) {
            Some(IgnoreReason::IgnoreRule)
        } else if whitelisted {
            None
        } else {
            relative
                .file_name()
                .and_then(|name| directory_policy(name, self.options.mode))
        };

        if let Some(reason) = reason {
            self.excluded_directories.push(relative.clone());
            self.record_ignored(relative, reason, None);
        } else {
            self.directories.push((absolute, relative));
        }
    }

    fn process_file(
        &mut self,
        absolute: &Path,
        relative: PathBuf,
        followed_metadata: Option<fs::Metadata>,
    ) {
        let whitelisted = self.rules.is_explicitly_whitelisted(&relative, false);
        if self.rules.is_ignored(&relative, false) {
            self.record_ignored(relative, IgnoreReason::IgnoreRule, None);
            return;
        }
        if !whitelisted
            && let Some(reason) = relative
                .file_name()
                .and_then(|name| file_policy(name, self.options.mode))
        {
            self.record_ignored(relative, reason, None);
            return;
        }

        let metadata = match followed_metadata {
            Some(metadata) => metadata,
            None => match fs::metadata(absolute) {
                Ok(metadata) => metadata,
                Err(error) => {
                    self.record_io(&relative, &error);
                    return;
                }
            },
        };
        let byte_len = metadata.len();
        if byte_len > self.options.max_file_bytes {
            self.record_ignored(relative, IgnoreReason::Oversized, None);
            return;
        }

        let Some(language) = self.languages.classify(&relative).cloned() else {
            self.record_ignored(relative, IgnoreReason::UnsupportedLanguage, None);
            return;
        };
        let absolute_path = match fs::canonicalize(absolute) {
            Ok(path) => path,
            Err(error) => {
                self.record_io(&relative, &error);
                return;
            }
        };
        self.files.push(DiscoveredFile {
            absolute_path,
            relative_path: relative,
            language,
            byte_len,
        });
    }

    fn record_io(&mut self, relative: &Path, error: &io::Error) {
        self.warnings
            .push(format!("{}: {error}", self.root.join(relative).display()));
        self.record_ignored(
            relative.to_path_buf(),
            IgnoreReason::Io,
            Some(error.to_string()),
        );
    }

    fn record_ignored(
        &mut self,
        relative_path: PathBuf,
        reason: IgnoreReason,
        detail: Option<String>,
    ) {
        self.ignored_total += 1;
        if self.options.collect_ignored {
            self.ignored.push(IgnoredPath {
                relative_path,
                reason,
                detail,
            });
        }
    }

    fn finish(mut self) -> DiscoveryReport {
        self.files
            .sort_by(|left, right| compare_paths(&left.relative_path, &right.relative_path));
        self.excluded_directories
            .sort_by(|left, right| compare_paths(left, right));
        self.excluded_directories.dedup();
        self.ignored.sort_by(|left, right| {
            compare_paths(&left.relative_path, &right.relative_path)
                .then_with(|| reason_rank(left.reason).cmp(&reason_rank(right.reason)))
                .then_with(|| left.detail.cmp(&right.detail))
        });
        self.ignored.truncate(MAX_IGNORED_DETAILS);
        self.warnings.sort();

        DiscoveryReport {
            root: self.root,
            files: self.files,
            excluded_directories: self.excluded_directories,
            ignored: self.ignored,
            ignored_total: self.ignored_total,
            warnings: self.warnings,
        }
    }
}

fn compare_paths(left: &Path, right: &Path) -> Ordering {
    normalized_path_bytes(left).cmp(&normalized_path_bytes(right))
}

fn normalized_path_bytes(path: &Path) -> Vec<u8> {
    let mut normalized = Vec::new();
    for (index, component) in path.components().enumerate() {
        if index != 0 {
            normalized.push(b'/');
        }
        normalized.extend_from_slice(component.as_os_str().as_encoded_bytes());
    }
    normalized
}

const fn reason_rank(reason: IgnoreReason) -> u8 {
    match reason {
        IgnoreReason::IgnoreRule => 0,
        IgnoreReason::DirectoryPolicy => 1,
        IgnoreReason::SuffixPolicy => 2,
        IgnoreReason::FilenamePolicy => 3,
        IgnoreReason::PatternPolicy => 4,
        IgnoreReason::Oversized => 5,
        IgnoreReason::UnsupportedLanguage => 6,
        IgnoreReason::Symlink => 7,
        IgnoreReason::Io => 8,
    }
}
