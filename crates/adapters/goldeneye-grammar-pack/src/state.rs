use super::{GrammarPackLock, GrammarPackState, PackError, Path, lock_file_hash};

impl GrammarPackState {
    /// Compute the state expected for a validated lock file.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] when the lock file cannot be opened or hashed.
    pub fn expected(
        lock_path: impl AsRef<Path>,
        lock: &GrammarPackLock,
    ) -> Result<Self, PackError> {
        Ok(Self {
            schema_version: 1,
            lock_hash: lock_file_hash(lock_path)?,
            upstream_commit: lock.upstream_commit().to_owned(),
            grammar_count: lock.grammars.len(),
            asset_count: lock.locked_asset_paths().count(),
        })
    }

    #[must_use]
    pub fn lock_hash(&self) -> &str {
        &self.lock_hash
    }
}
