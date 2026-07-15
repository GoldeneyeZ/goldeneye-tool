use super::{
    BTreeSet, GrammarPackLock, LanguageBindingStatus, PackError, invalid, require_nonempty,
    validate_asset_path, validate_component, validate_exported_symbol, validate_hash,
    validate_relative_path, validate_sorted_unique,
};

impl GrammarPackLock {
    pub(super) fn validate(&self) -> Result<(), PackError> {
        self.validate_header()?;
        let grammar_names = self.validate_grammars()?;
        self.validate_native_support(&grammar_names)?;
        let bound_grammars = self.validate_language_mappings(&grammar_names)?;
        self.validate_binding_states(&bound_grammars)
    }

    fn validate_header(&self) -> Result<(), PackError> {
        if self.schema_version != 1 {
            return invalid(format!(
                "unsupported schema_version {}",
                self.schema_version
            ));
        }
        require_nonempty("upstream_repository", &self.upstream_repository)?;
        require_nonempty("upstream_commit", &self.upstream_commit)?;
        if self.hash_algorithm != "sha256" {
            return invalid("hash_algorithm must be sha256");
        }
        if self.hash_domain != "goldeneye-grammar-assets-v1" {
            return invalid("hash_domain must be goldeneye-grammar-assets-v1");
        }
        if self.compatible_abi_min > self.compatible_abi_max {
            return invalid("compatible ABI range is reversed");
        }
        if self.declared_grammar_count != self.grammars.len() {
            return invalid(format!(
                "declared grammar count {} does not match {} records",
                self.declared_grammar_count,
                self.grammars.len()
            ));
        }
        if self.declared_language_binding_count != self.language_mappings.len() {
            return invalid(format!(
                "declared language-binding count {} does not match {} records",
                self.declared_language_binding_count,
                self.language_mappings.len()
            ));
        }
        if self.declared_native_support_count != self.native_support.len() {
            return invalid(format!(
                "declared native-support count {} does not match {} records",
                self.declared_native_support_count,
                self.native_support.len()
            ));
        }

        Ok(())
    }

    fn validate_grammars(&self) -> Result<BTreeSet<String>, PackError> {
        let mut grammar_names = BTreeSet::new();
        let mut exported_symbols = BTreeSet::new();
        let mut destination_paths = BTreeSet::new();
        for grammar in &self.grammars {
            validate_component(&grammar.name)?;
            if !grammar_names.insert(grammar.name.clone()) {
                return invalid(format!("duplicate grammar name {}", grammar.name));
            }
            require_nonempty("grammar repository", &grammar.repository)?;
            match (&grammar.commit, &grammar.missing_commit_reason) {
                (Some(commit), None) => require_nonempty("grammar commit", commit)?,
                (None, Some(reason)) => require_nonempty("missing commit reason", reason)?,
                _ => {
                    return invalid(format!(
                        "grammar {} must declare exactly one of commit or missing_commit_reason",
                        grammar.name
                    ));
                }
            }
            if !(self.compatible_abi_min..=self.compatible_abi_max).contains(&grammar.abi) {
                return invalid(format!(
                    "grammar {} ABI {} is outside {}..={}",
                    grammar.name, grammar.abi, self.compatible_abi_min, self.compatible_abi_max
                ));
            }
            validate_exported_symbol(&grammar.exported_symbol)?;
            if !exported_symbols.insert(grammar.exported_symbol.clone()) {
                return invalid(format!(
                    "duplicate exported symbol {}",
                    grammar.exported_symbol
                ));
            }
            if !matches!(grammar.scanner_language.as_str(), "none" | "c") {
                return invalid(format!(
                    "grammar {} has unsupported scanner language {}",
                    grammar.name, grammar.scanner_language
                ));
            }
            require_nonempty("verdict", &grammar.verdict)?;
            validate_hash(&grammar.source_hash)?;
            validate_sorted_unique("asset", &grammar.assets)?;
            validate_sorted_unique("license", &grammar.license_files)?;
            if grammar.assets.is_empty() {
                return invalid(format!("grammar {} has no assets", grammar.name));
            }
            if grammar.license_files.is_empty() {
                return invalid(format!("grammar {} has no license files", grammar.name));
            }
            if grammar.license_files.as_slice() != ["LICENSE"] {
                return invalid(format!(
                    "grammar {} must declare exactly one direct LICENSE",
                    grammar.name
                ));
            }
            if !grammar.assets.iter().any(|asset| asset == "parser.c") {
                return invalid(format!(
                    "grammar {} must lock its direct parser.c",
                    grammar.name
                ));
            }
            let assets = grammar.assets.iter().collect::<BTreeSet<_>>();
            for asset in &grammar.assets {
                validate_asset_path(asset)?;
                let destination = format!("{}/{}", grammar.name, asset).to_lowercase();
                if !destination_paths.insert(destination) {
                    return invalid(format!(
                        "case-folded destination collision at {}/{}",
                        grammar.name, asset
                    ));
                }
            }
            for license in &grammar.license_files {
                validate_relative_path(license)?;
                if !assets.contains(license) {
                    return invalid(format!(
                        "grammar {} license {} is not a locked asset",
                        grammar.name, license
                    ));
                }
            }
            for note in &grammar.provenance_notes {
                require_nonempty("provenance note", note)?;
            }
            if let Some(reason) = &grammar.orphan_reason {
                require_nonempty("orphan reason", reason)?;
            }
        }

        Ok(grammar_names)
    }

    fn validate_native_support(&self, grammar_names: &BTreeSet<String>) -> Result<(), PackError> {
        let mut group_names = grammar_names
            .iter()
            .map(|name| name.to_lowercase())
            .collect::<BTreeSet<_>>();
        let mut destination_paths = BTreeSet::new();
        for support in &self.native_support {
            validate_component(&support.name)?;
            if !group_names.insert(support.name.to_lowercase()) {
                return invalid(format!(
                    "duplicate or case-folded native-support group {}",
                    support.name
                ));
            }
            require_nonempty("native-support repository", &support.repository)?;
            match (&support.commit, &support.missing_commit_reason) {
                (Some(commit), None) => require_nonempty("native-support commit", commit)?,
                (None, Some(reason)) => {
                    require_nonempty("native-support missing commit reason", reason)?;
                }
                _ => {
                    return invalid(format!(
                        "native-support group {} must declare exactly one of commit or missing_commit_reason",
                        support.name
                    ));
                }
            }
            if support.hash_domain != "goldeneye-native-support-assets-v1" {
                return invalid(format!(
                    "native-support group {} must use goldeneye-native-support-assets-v1",
                    support.name
                ));
            }
            require_nonempty("native-support verdict", &support.verdict)?;
            validate_hash(&support.source_hash)?;
            validate_sorted_unique("native-support asset", &support.assets)?;
            validate_sorted_unique("native-support license", &support.license_files)?;
            if support.assets.is_empty() {
                return invalid(format!(
                    "native-support group {} has no assets",
                    support.name
                ));
            }
            if support.license_files.is_empty() {
                return invalid(format!(
                    "native-support group {} has no license files",
                    support.name
                ));
            }
            let assets = support.assets.iter().collect::<BTreeSet<_>>();
            for asset in &support.assets {
                validate_relative_path(asset)?;
                let destination = format!("{}/{}", support.name, asset).to_lowercase();
                if !destination_paths.insert(destination) {
                    return invalid(format!(
                        "case-folded native-support destination collision at {}/{}",
                        support.name, asset
                    ));
                }
            }
            for license in &support.license_files {
                validate_relative_path(license)?;
                if !assets.contains(license) {
                    return invalid(format!(
                        "native-support group {} license {} is not a locked asset",
                        support.name, license
                    ));
                }
            }
            for note in &support.provenance_notes {
                require_nonempty("native-support provenance note", note)?;
            }
        }

        Ok(())
    }

    fn validate_language_mappings(
        &self,
        grammar_names: &BTreeSet<String>,
    ) -> Result<BTreeSet<String>, PackError> {
        let mut language_ids = BTreeSet::new();
        let mut bound_grammars = BTreeSet::new();
        for mapping in &self.language_mappings {
            validate_component(&mapping.language_id)?;
            if !language_ids.insert(mapping.language_id.clone()) {
                return invalid(format!("duplicate language id {}", mapping.language_id));
            }
            match mapping.status {
                LanguageBindingStatus::Available => {
                    let grammar = mapping.grammar.as_deref().ok_or_else(|| {
                        PackError::Invalid(format!(
                            "available language {} has no grammar",
                            mapping.language_id
                        ))
                    })?;
                    if mapping.reason.is_some() {
                        return invalid(format!(
                            "available language {} must not have an unavailable reason",
                            mapping.language_id
                        ));
                    }
                    if !grammar_names.contains(grammar) {
                        return invalid(format!(
                            "language {} references unknown grammar {grammar}",
                            mapping.language_id
                        ));
                    }
                    bound_grammars.insert(grammar.to_owned());
                }
                LanguageBindingStatus::Unavailable => {
                    if mapping.grammar.is_some() {
                        return invalid(format!(
                            "unavailable language {} must not name a grammar",
                            mapping.language_id
                        ));
                    }
                    require_nonempty(
                        "unavailable reason",
                        mapping.reason.as_deref().unwrap_or_default(),
                    )?;
                }
            }
        }

        Ok(bound_grammars)
    }

    fn validate_binding_states(&self, bound_grammars: &BTreeSet<String>) -> Result<(), PackError> {
        for grammar in &self.grammars {
            let bound = bound_grammars.contains(grammar.name.as_str());
            if bound == grammar.orphan_reason.is_some() {
                return invalid(format!(
                    "grammar {} must be explicitly either bound or orphaned",
                    grammar.name
                ));
            }
        }

        Ok(())
    }
}
