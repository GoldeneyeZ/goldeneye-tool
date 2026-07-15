use super::{
    BTreeMap, BTreeSet, BUFFER_SIZE, BufReader, GrammarPackLock, LanguageBindingStatus, PackError,
    Path, Read, lock_bytes_hash, open_regular_file,
};

impl GrammarPackLock {
    /// Load and validate a grammar-pack lock from TOML.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] when the file cannot be read, TOML cannot be
    /// decoded, or any lock invariant fails.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PackError> {
        Self::load_with_hash(path).map(|(lock, _hash)| lock)
    }

    /// Load and validate a grammar-pack lock while hashing the same bytes.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] when the exact regular-file bytes cannot be read,
    /// are not UTF-8, TOML cannot be decoded, or any lock invariant fails.
    pub fn load_with_hash(path: impl AsRef<Path>) -> Result<(Self, String), PackError> {
        let path = path.as_ref();
        let file = open_regular_file(path)?;
        let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|source| PackError::Io {
                path: path.to_path_buf(),
                source,
            })?;
        let hash = lock_bytes_hash(&bytes);
        let source = String::from_utf8(bytes).map_err(|source| PackError::Utf8 {
            path: path.to_path_buf(),
            source,
        })?;
        Ok((Self::parse(&source)?, hash))
    }

    /// Parse and validate a grammar-pack lock from TOML text.
    ///
    /// # Errors
    ///
    /// Returns [`PackError`] when TOML decoding or lock validation fails.
    pub fn parse(source: &str) -> Result<Self, PackError> {
        let lock: Self = toml::from_str(source)?;
        lock.validate()?;
        Ok(lock)
    }

    #[must_use]
    pub fn upstream_commit(&self) -> &str {
        &self.upstream_commit
    }

    #[must_use]
    pub fn upstream_repository(&self) -> &str {
        &self.upstream_repository
    }

    #[must_use]
    pub fn abi_histogram(&self) -> BTreeMap<u32, usize> {
        let mut histogram = BTreeMap::new();
        for grammar in &self.grammars {
            *histogram.entry(grammar.abi).or_insert(0) += 1;
        }
        histogram
    }

    #[must_use]
    pub fn available_language_count(&self) -> usize {
        self.language_mappings
            .iter()
            .filter(|mapping| mapping.status == LanguageBindingStatus::Available)
            .count()
    }

    #[must_use]
    pub fn unique_bound_grammar_count(&self) -> usize {
        self.language_mappings
            .iter()
            .filter_map(|mapping| mapping.grammar.as_deref())
            .collect::<BTreeSet<_>>()
            .len()
    }

    #[must_use]
    pub fn unavailable_language_ids(&self) -> Vec<&str> {
        self.language_mappings
            .iter()
            .filter(|mapping| mapping.status == LanguageBindingStatus::Unavailable)
            .map(|mapping| mapping.language_id.as_str())
            .collect()
    }

    #[must_use]
    pub fn orphan_grammar_names(&self) -> Vec<&str> {
        self.grammars
            .iter()
            .filter(|grammar| grammar.orphan_reason.is_some())
            .map(|grammar| grammar.name.as_str())
            .collect()
    }

    #[must_use]
    pub fn grammar_name_for(&self, language_id: &str) -> Option<&str> {
        self.language_mappings
            .iter()
            .find(|mapping| mapping.language_id == language_id)
            .and_then(|mapping| mapping.grammar.as_deref())
    }

    pub fn locked_asset_paths(&self) -> impl Iterator<Item = String> + '_ {
        self.grammars
            .iter()
            .flat_map(|grammar| {
                grammar
                    .assets
                    .iter()
                    .map(move |asset| format!("{}/{asset}", grammar.name))
            })
            .chain(self.native_support.iter().flat_map(|support| {
                support
                    .assets
                    .iter()
                    .map(move |asset| format!("{}/{asset}", support.name))
            }))
    }
}
