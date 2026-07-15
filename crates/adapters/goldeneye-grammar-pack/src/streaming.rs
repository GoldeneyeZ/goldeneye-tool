use super::{
    GrammarPackLock, PackError, Path, SourceSession, VerifiedPack, ensure_safe_existing_directory,
    invalid, stream_grammar_assets, stream_native_support_assets, validate_relative_path,
};

impl GrammarPackLock {
    /// Verify every locked source asset and grammar hash.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] for unsafe paths, missing/non-regular assets,
    /// I/O failures, or content-hash mismatches.
    pub fn verify_source(&self, source_root: impl AsRef<Path>) -> Result<VerifiedPack, PackError> {
        let mut grammar_source = SourceSession::directory(source_root.as_ref())?;
        let mut support_source = SourceSession::directory(source_root.as_ref())?;
        self.stream_assets(&mut grammar_source, &mut support_source, None)
    }

    /// Verify every locked asset from the lock's exact upstream Git commit.
    ///
    /// `git_prefix` names the grammar root inside that commit. The commit is
    /// always taken from [`Self::upstream_commit`]; callers cannot substitute a
    /// different revision.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] for an unsafe repository/prefix, missing or
    /// non-regular Git entries, malformed Git protocol output, I/O failures, or
    /// content-hash mismatches.
    pub fn verify_git_source(
        &self,
        git_repository: impl AsRef<Path>,
        git_prefix: &str,
    ) -> Result<VerifiedPack, PackError> {
        let repository = git_repository.as_ref();
        let mut grammar_source =
            SourceSession::git(repository, git_prefix, self.upstream_commit())?;
        let support_prefix = self.native_support_git_prefix(git_prefix)?;
        let mut support_source =
            SourceSession::git(repository, &support_prefix, self.upstream_commit())?;
        self.stream_assets(&mut grammar_source, &mut support_source, None)
    }

    /// Copy locked assets while hashing the same open source handles.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] for unsafe paths, pre-existing destination
    /// files, I/O failures, or content-hash mismatches.
    pub fn copy_verified_assets(
        &self,
        source_root: impl AsRef<Path>,
        destination_root: impl AsRef<Path>,
    ) -> Result<VerifiedPack, PackError> {
        let mut grammar_source = SourceSession::directory(source_root.as_ref())?;
        let mut support_source = SourceSession::directory(source_root.as_ref())?;
        self.stream_assets(
            &mut grammar_source,
            &mut support_source,
            Some(destination_root.as_ref()),
        )
    }

    /// Copy locked assets from the exact upstream Git commit while hashing the
    /// same blob streams.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] for unsafe paths, missing/non-regular Git entries,
    /// malformed Git protocol output, pre-existing destination files, I/O
    /// failures, or content-hash mismatches.
    pub fn copy_verified_git_assets(
        &self,
        git_repository: impl AsRef<Path>,
        git_prefix: &str,
        destination_root: impl AsRef<Path>,
    ) -> Result<VerifiedPack, PackError> {
        let repository = git_repository.as_ref();
        let mut grammar_source =
            SourceSession::git(repository, git_prefix, self.upstream_commit())?;
        let support_prefix = self.native_support_git_prefix(git_prefix)?;
        let mut support_source =
            SourceSession::git(repository, &support_prefix, self.upstream_commit())?;
        self.stream_assets(
            &mut grammar_source,
            &mut support_source,
            Some(destination_root.as_ref()),
        )
    }

    fn stream_assets(
        &self,
        grammar_source: &mut SourceSession,
        support_source: &mut SourceSession,
        destination_root: Option<&Path>,
    ) -> Result<VerifiedPack, PackError> {
        if let Some(destination_root) = destination_root {
            ensure_safe_existing_directory(destination_root)?;
        }

        let mut asset_count = 0;
        for grammar in &self.grammars {
            let actual = stream_grammar_assets(grammar, grammar_source, destination_root)?;
            if actual != grammar.source_hash {
                return Err(PackError::HashMismatch {
                    grammar: grammar.name.clone(),
                    expected: grammar.source_hash.clone(),
                    actual,
                });
            }
            asset_count += grammar.assets.len();
        }
        for support in &self.native_support {
            let actual = stream_native_support_assets(support, support_source, destination_root)?;
            if actual != support.source_hash {
                return Err(PackError::HashMismatch {
                    grammar: support.name.clone(),
                    expected: support.source_hash.clone(),
                    actual,
                });
            }
            asset_count += support.assets.len();
        }
        grammar_source.finish()?;
        support_source.finish()?;

        Ok(VerifiedPack {
            grammar_count: self.grammars.len(),
            asset_count,
        })
    }

    fn native_support_git_prefix(&self, grammar_prefix: &str) -> Result<String, PackError> {
        if self.native_support.is_empty() {
            return Ok(grammar_prefix.to_owned());
        }
        let (parent, leaf) = grammar_prefix.rsplit_once('/').ok_or_else(|| {
            PackError::Invalid(
                "native support requires a Git grammar prefix ending in /grammars".into(),
            )
        })?;
        if parent.is_empty() || leaf != "grammars" {
            return invalid("native support requires a Git grammar prefix ending in /grammars");
        }
        validate_relative_path(parent)?;
        Ok(parent.to_owned())
    }
}
