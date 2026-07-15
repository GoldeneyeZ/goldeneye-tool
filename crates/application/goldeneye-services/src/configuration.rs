use std::{
    env,
    path::{Path, PathBuf},
};

use crate::ServiceError;

pub const DATABASE_PATH_ENV: &str = "GOLDENEYE_DB_PATH";
pub const PROJECT_ROOT_ENV: &str = "GOLDENEYE_PROJECT_ROOT";
pub const ALLOWED_ROOT_ENV: &str = "CBM_ALLOWED_ROOT";
pub const SEMANTIC_ENABLED_ENV: &str = "CBM_SEMANTIC_ENABLED";
pub const SEMANTIC_THRESHOLD_ENV: &str = "CBM_SEMANTIC_THRESHOLD";
pub const DEFAULT_SEMANTIC_THRESHOLD: f32 = 0.75;

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceConfig {
    pub(crate) database_path: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) allowed_root: Option<PathBuf>,
    semantic_enabled: bool,
    semantic_threshold: f32,
}

impl ServiceConfig {
    #[must_use]
    pub fn new(database_path: impl Into<PathBuf>, project_root: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
            project_root: project_root.into(),
            allowed_root: None,
            semantic_enabled: false,
            semantic_threshold: DEFAULT_SEMANTIC_THRESHOLD,
        }
    }

    #[must_use]
    pub fn with_allowed_root(mut self, allowed_root: impl Into<PathBuf>) -> Self {
        self.allowed_root = Some(allowed_root.into());
        self
    }

    #[must_use]
    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    #[must_use]
    pub fn allowed_root(&self) -> Option<&Path> {
        self.allowed_root.as_deref()
    }

    #[must_use]
    pub const fn semantic_enabled(&self) -> bool {
        self.semantic_enabled
    }

    #[must_use]
    pub const fn semantic_threshold(&self) -> f32 {
        self.semantic_threshold
    }

    #[must_use]
    pub fn with_semantic_config(mut self, enabled: bool, threshold: f32) -> Self {
        self.semantic_enabled = enabled;
        self.semantic_threshold = if valid_semantic_threshold(threshold) {
            threshold
        } else {
            DEFAULT_SEMANTIC_THRESHOLD
        };
        self
    }

    /// Builds configuration from process environment without opening the database.
    ///
    /// # Errors
    ///
    /// Returns a typed configuration error when the current directory cannot be read.
    pub fn from_env() -> Result<Self, ServiceError> {
        let project_root = env::var_os(PROJECT_ROOT_ENV).map_or_else(
            || env::current_dir().map_err(|source| ServiceError::CurrentDirectory { source }),
            |value| Ok(PathBuf::from(value)),
        )?;
        let database_path =
            env::var_os(DATABASE_PATH_ENV).map_or_else(default_database_path, PathBuf::from);
        let mut config = Self::new(database_path, project_root).with_semantic_config(
            semantic_enabled_from_value(env::var_os(SEMANTIC_ENABLED_ENV)),
            semantic_threshold_from_value(env::var_os(SEMANTIC_THRESHOLD_ENV)),
        );
        if let Some(value) = env::var_os(ALLOWED_ROOT_ENV) {
            config = config.with_allowed_root(PathBuf::from(value));
        }
        Ok(config)
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self::from_env().unwrap_or_else(|_| Self::new(default_database_path(), "."))
    }
}

fn default_database_path() -> PathBuf {
    if let Some(path) = env::var_os("CBM_CACHE_DIR") {
        return PathBuf::from(path).join("goldeneye.db");
    }
    if let Some(path) = env::var_os("LOCALAPPDATA") {
        return PathBuf::from(path)
            .join("codebase-memory-mcp")
            .join("goldeneye.db");
    }
    if let Some(path) = env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path)
            .join("codebase-memory-mcp")
            .join("goldeneye.db");
    }
    if let Some(path) = env::var_os("HOME") {
        return PathBuf::from(path)
            .join(".cache")
            .join("codebase-memory-mcp")
            .join("goldeneye.db");
    }
    PathBuf::from(".goldeneye/goldeneye.db")
}

fn semantic_enabled_from_value(value: Option<std::ffi::OsString>) -> bool {
    value.is_some_and(|value| value.to_string_lossy().starts_with('1'))
}

fn semantic_threshold_from_value(value: Option<std::ffi::OsString>) -> f32 {
    value
        .and_then(|value| value.to_string_lossy().parse::<f32>().ok())
        .filter(|value| valid_semantic_threshold(*value))
        .unwrap_or(DEFAULT_SEMANTIC_THRESHOLD)
}

fn valid_semantic_threshold(value: f32) -> bool {
    value > 0.0 && value <= 1.0
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)]

    use std::ffi::OsString;

    use super::{
        DEFAULT_SEMANTIC_THRESHOLD, semantic_enabled_from_value, semantic_threshold_from_value,
    };

    #[test]
    fn upstream_semantic_environment_parsing_is_exact() {
        assert!(!semantic_enabled_from_value(None));
        assert!(!semantic_enabled_from_value(Some(OsString::from("true"))));
        assert!(semantic_enabled_from_value(Some(OsString::from("1"))));
        assert!(semantic_enabled_from_value(Some(OsString::from(
            "1-enabled"
        ))));

        assert_eq!(
            semantic_threshold_from_value(None),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("invalid"))),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("0"))),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("1.1"))),
            DEFAULT_SEMANTIC_THRESHOLD
        );
        assert_eq!(
            semantic_threshold_from_value(Some(OsString::from("0.82"))),
            0.82
        );
    }
}
