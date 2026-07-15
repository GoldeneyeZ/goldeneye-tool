use std::ffi::OsStr;

use crate::{IgnoreReason, IndexMode};

const SAFETY_CORE_DIRS: [&str; 4] = [".git", "node_modules", ".worktrees", ".claude-worktrees"];

const ALWAYS_SKIP_DIRS: [&str; 73] = [
    ".git",
    ".hg",
    ".svn",
    ".worktrees",
    ".idea",
    ".vs",
    ".vscode",
    ".eclipse",
    ".claude",
    ".claude-worktrees",
    "Antigravity",
    ".cache",
    ".eggs",
    ".env",
    ".mypy_cache",
    ".nox",
    ".pytest_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
    "__pycache__",
    "env",
    "htmlcov",
    "site-packages",
    "venv",
    ".npm",
    ".nyc_output",
    ".pnpm-store",
    ".yarn",
    "bower_components",
    "coverage",
    "node_modules",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".angular",
    ".turbo",
    ".parcel-cache",
    ".docusaurus",
    ".expo",
    "dist",
    "obj",
    "Pods",
    "target",
    "temp",
    "tmp",
    ".terraform",
    ".serverless",
    "bazel-bin",
    "bazel-out",
    "bazel-testlogs",
    ".cargo",
    ".stack-work",
    ".dart_tool",
    "zig-cache",
    "zig-out",
    ".metals",
    ".bloop",
    ".bsp",
    ".ccls-cache",
    ".clangd",
    "elm-stuff",
    "_opam",
    ".cpcache",
    ".shadow-cljs",
    ".vercel",
    ".netlify",
    "deploy",
    "deployed",
    ".qdrant_code_embeddings",
    ".tmp",
    "vendor",
    "vendored",
];

const FAST_SKIP_DIRS: [&str; 40] = [
    "generated",
    "gen",
    "auto-generated",
    "fixtures",
    "testdata",
    "test_data",
    "__tests__",
    "__mocks__",
    "__snapshots__",
    "__fixtures__",
    "__test__",
    "docs",
    "doc",
    "documentation",
    "examples",
    "example",
    "samples",
    "sample",
    "assets",
    "static",
    "public",
    "media",
    "third_party",
    "thirdparty",
    "3rdparty",
    "external",
    "migrations",
    "seeds",
    "e2e",
    "integration",
    "locale",
    "locales",
    "i18n",
    "l10n",
    "scripts",
    "tools",
    "hack",
    "bin",
    "build",
    "out",
];

const ALWAYS_IGNORED_SUFFIXES: [&str; 31] = [
    ".tmp", "~", ".pyc", ".pyo", ".o", ".a", ".so", ".dll", ".class", ".png", ".jpg", ".jpeg",
    ".gif", ".ico", ".bmp", ".tiff", ".webp", ".svg", ".wasm", ".node", ".exe", ".bin", ".dat",
    ".db", ".sqlite", ".sqlite3", ".woff", ".woff2", ".ttf", ".eot", ".otf",
];

const FAST_IGNORED_SUFFIXES: [&str; 47] = [
    ".zip",
    ".tar",
    ".gz",
    ".bz2",
    ".xz",
    ".rar",
    ".7z",
    ".jar",
    ".war",
    ".ear",
    ".mp3",
    ".mp4",
    ".avi",
    ".mov",
    ".wav",
    ".flac",
    ".ogg",
    ".mkv",
    ".webm",
    ".pdf",
    ".doc",
    ".docx",
    ".xls",
    ".xlsx",
    ".ppt",
    ".pptx",
    ".odt",
    ".ods",
    ".map",
    ".min.js",
    ".min.css",
    ".pem",
    ".crt",
    ".key",
    ".cer",
    ".p12",
    ".pb",
    ".avro",
    ".parquet",
    ".beam",
    ".elc",
    ".rlib",
    ".coverage",
    ".prof",
    ".out",
    ".patch",
    ".diff",
];

const FAST_SKIP_FILENAMES: [&str; 34] = [
    "LICENSE",
    "LICENSE.txt",
    "LICENSE.md",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "LICENCE",
    "LICENCE.txt",
    "LICENCE.md",
    "CHANGELOG",
    "CHANGELOG.md",
    "CHANGES.md",
    "HISTORY",
    "HISTORY.md",
    "AUTHORS",
    "AUTHORS.md",
    "CONTRIBUTORS",
    "CONTRIBUTORS.md",
    "CODEOWNERS",
    "go.sum",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Pipfile.lock",
    "poetry.lock",
    "Gemfile.lock",
    "Cargo.lock",
    "mix.lock",
    "flake.lock",
    "pubspec.lock",
    "composer.lock",
    "package-lock.json",
    "configure",
    "Makefile.in",
    "config.guess",
    "config.sub",
];

const FAST_PATTERNS: [&str; 15] = [
    ".d.ts",
    ".bundle.",
    ".chunk.",
    ".generated.",
    ".pb.go",
    "_pb2.py",
    ".pb2.py",
    "_grpc.pb.go",
    "_string.go",
    "mock_",
    "_mock.",
    "_test_helpers.",
    ".stories.",
    ".spec.",
    ".test.",
];

const IGNORED_JSON_FILES: [&str; 29] = [
    "package.json",
    "package-lock.json",
    "tsconfig.json",
    "jsconfig.json",
    "composer.json",
    "composer.lock",
    "yarn.lock",
    "openapi.json",
    "swagger.json",
    "jest.config.json",
    ".eslintrc.json",
    ".prettierrc.json",
    ".babelrc.json",
    "tslint.json",
    "angular.json",
    "firebase.json",
    "renovate.json",
    "lerna.json",
    "turbo.json",
    ".stylelintrc.json",
    "pnpm-lock.json",
    "deno.json",
    "biome.json",
    "devcontainer.json",
    ".devcontainer.json",
    "launch.json",
    "settings.json",
    "extensions.json",
    "tasks.json",
];

#[must_use]
pub fn directory_policy(name: &OsStr, mode: IndexMode) -> Option<IgnoreReason> {
    if contains_name(&ALWAYS_SKIP_DIRS, name)
        || (mode != IndexMode::Full && contains_name(&FAST_SKIP_DIRS, name))
    {
        Some(IgnoreReason::DirectoryPolicy)
    } else {
        None
    }
}

pub(crate) fn is_safety_core_directory(name: &OsStr) -> bool {
    contains_name(&SAFETY_CORE_DIRS, name)
}

#[must_use]
pub fn file_policy(name: &OsStr, mode: IndexMode) -> Option<IgnoreReason> {
    let encoded = name.as_encoded_bytes();
    if has_suffix(&ALWAYS_IGNORED_SUFFIXES, encoded) {
        return Some(IgnoreReason::SuffixPolicy);
    }
    if contains_name(&IGNORED_JSON_FILES, name) {
        return Some(IgnoreReason::FilenamePolicy);
    }
    if mode == IndexMode::Full {
        return None;
    }
    if has_suffix(&FAST_IGNORED_SUFFIXES, encoded) {
        return Some(IgnoreReason::SuffixPolicy);
    }
    if contains_name(&FAST_SKIP_FILENAMES, name) {
        return Some(IgnoreReason::FilenamePolicy);
    }
    if FAST_PATTERNS
        .iter()
        .any(|pattern| contains(encoded, pattern.as_bytes()))
    {
        return Some(IgnoreReason::PatternPolicy);
    }
    None
}

fn contains_name<const N: usize>(table: &[&str; N], name: &OsStr) -> bool {
    table.iter().any(|candidate| name == OsStr::new(candidate))
}

fn has_suffix<const N: usize>(table: &[&str; N], name: &[u8]) -> bool {
    table.iter().any(|suffix| name.ends_with(suffix.as_bytes()))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_tables_have_audited_upstream_counts() {
        assert_eq!(ALWAYS_SKIP_DIRS.len(), 73);
        assert_eq!(FAST_SKIP_DIRS.len(), 40);
        assert_eq!(ALWAYS_IGNORED_SUFFIXES.len(), 31);
        assert_eq!(FAST_IGNORED_SUFFIXES.len(), 47);
        assert_eq!(FAST_SKIP_FILENAMES.len(), 34);
        assert_eq!(FAST_PATTERNS.len(), 15);
        assert_eq!(IGNORED_JSON_FILES.len(), 29);
    }
}
