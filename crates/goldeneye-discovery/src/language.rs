use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::Path;
use std::sync::OnceLock;

use crate::{DiscoveryError, LanguageId};

const UPSTREAM_LANGUAGES: &str = include_str!("../data/languages.tsv");
const HEADER: &str = "id\tdisplay_name\textensions\tfilenames\tcompound_extensions";

static REGISTRY: OnceLock<LanguageRegistry> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSpec {
    id: LanguageId,
    display_name: String,
}

impl LanguageSpec {
    #[must_use]
    pub fn id(&self) -> &LanguageId {
        &self.id
    }

    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[derive(Debug, Clone)]
pub struct LanguageRegistry {
    extensions: HashMap<OsString, LanguageId>,
    filenames: HashMap<OsString, LanguageId>,
    compound_extensions: Vec<(OsString, LanguageId)>,
    specifications: HashMap<LanguageId, LanguageSpec>,
    overrides: HashMap<OsString, LanguageId>,
}

impl LanguageRegistry {
    /// Returns the immutable registry derived from the audited upstream tables.
    ///
    /// # Panics
    ///
    /// Panics when the checked-in generated TSV is invalid. Reproducibility and
    /// parity tests guard this build-time invariant.
    #[must_use]
    pub fn upstream() -> &'static Self {
        REGISTRY.get_or_init(|| {
            Self::parse(UPSTREAM_LANGUAGES)
                .expect("checked-in upstream language registry must remain valid")
        })
    }

    /// Creates an upstream registry with explicit extension overrides.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryError::InvalidLanguageData`] when an override key does
    /// not start with a dot or references a language absent from the registry.
    pub fn with_overrides(
        overrides: HashMap<OsString, LanguageId>,
    ) -> Result<Self, DiscoveryError> {
        let mut registry = Self::upstream().clone();
        for (extension, language) in &overrides {
            if !extension.as_encoded_bytes().starts_with(b".") {
                return Err(invalid_data(
                    0,
                    format!(
                        "override extension {} must start with '.'",
                        Path::new(extension).display()
                    ),
                ));
            }
            if !registry.specifications.contains_key(language) {
                return Err(invalid_data(
                    0,
                    format!("override references unknown language {}", language.as_str()),
                ));
            }
        }
        registry.overrides = overrides;
        Ok(registry)
    }

    #[must_use]
    pub fn classify(&self, path: &Path) -> Option<&LanguageId> {
        let filename = path.file_name()?;

        self.matching_override(filename)
            .or_else(|| self.filenames.get(filename))
            .or_else(|| self.matching_compound_extension(filename))
            .or_else(|| {
                let extension = extension_with_dot(path)?;
                self.extensions.get(extension.as_os_str())
            })
    }

    #[must_use]
    pub fn language_count(&self) -> usize {
        self.specifications.len()
    }

    #[must_use]
    pub fn extension_count(&self) -> usize {
        self.extensions.len()
    }

    #[must_use]
    pub fn filename_count(&self) -> usize {
        self.filenames.len()
    }

    #[must_use]
    pub fn compound_extension_count(&self) -> usize {
        self.compound_extensions.len()
    }

    #[must_use]
    pub fn specification(&self, id: &LanguageId) -> Option<&LanguageSpec> {
        self.specifications.get(id)
    }

    fn parse(data: &str) -> Result<Self, DiscoveryError> {
        let mut extensions = HashMap::new();
        let mut filenames = HashMap::new();
        let mut compound_extensions = Vec::new();
        let mut specifications = HashMap::new();
        let mut saw_header = false;

        for (index, line) in data.lines().enumerate() {
            let line_number = index + 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !saw_header {
                if line != HEADER {
                    return Err(invalid_data(line_number, "unexpected TSV header"));
                }
                saw_header = true;
                continue;
            }

            let columns: Vec<_> = line.split('\t').collect();
            if columns.len() != 5 {
                return Err(invalid_data(
                    line_number,
                    format!("expected 5 columns, found {}", columns.len()),
                ));
            }
            if columns[1].is_empty() {
                return Err(invalid_data(line_number, "display name cannot be empty"));
            }

            let id = LanguageId::new(columns[0].to_owned())
                .map_err(|_| invalid_data(line_number, "language identifier cannot be empty"))?;
            let specification = LanguageSpec {
                id: id.clone(),
                display_name: columns[1].to_owned(),
            };
            if specifications.insert(id.clone(), specification).is_some() {
                return Err(invalid_data(
                    line_number,
                    format!("duplicate language identifier {}", id.as_str()),
                ));
            }

            insert_mappings(&mut extensions, columns[2], &id, line_number, "extension")?;
            insert_mappings(&mut filenames, columns[3], &id, line_number, "filename")?;
            for extension in split_values(columns[4]) {
                compound_extensions.push((OsString::from(extension), id.clone()));
            }
        }

        if !saw_header {
            return Err(invalid_data(0, "TSV header is missing"));
        }
        compound_extensions.sort_by(|left, right| {
            right
                .0
                .as_encoded_bytes()
                .len()
                .cmp(&left.0.as_encoded_bytes().len())
                .then_with(|| left.0.cmp(&right.0))
        });

        Ok(Self {
            extensions,
            filenames,
            compound_extensions,
            specifications,
            overrides: HashMap::new(),
        })
    }

    fn matching_override(&self, filename: &OsStr) -> Option<&LanguageId> {
        let filename = filename.to_str()?;
        filename
            .match_indices('.')
            .map(|(index, _)| OsStr::new(&filename[index..]))
            .find_map(|extension| self.overrides.get(extension))
    }

    fn matching_compound_extension(&self, filename: &OsStr) -> Option<&LanguageId> {
        let filename = filename.to_str()?;
        self.compound_extensions
            .iter()
            .find(|(extension, _)| {
                extension
                    .to_str()
                    .is_some_and(|extension| filename.ends_with(extension))
            })
            .map(|(_, language)| language)
    }
}

fn split_values(values: &str) -> impl Iterator<Item = &str> {
    values.split(',').filter(|value| !value.is_empty())
}

fn insert_mappings(
    mappings: &mut HashMap<OsString, LanguageId>,
    values: &str,
    language: &LanguageId,
    line: usize,
    kind: &str,
) -> Result<(), DiscoveryError> {
    for value in split_values(values) {
        if mappings
            .insert(OsString::from(value), language.clone())
            .is_some()
        {
            return Err(invalid_data(line, format!("duplicate {kind} {value}")));
        }
    }
    Ok(())
}

fn extension_with_dot(path: &Path) -> Option<OsString> {
    let extension = path.extension()?;
    let mut with_dot = OsString::from(".");
    with_dot.push(extension);
    Some(with_dot)
}

fn invalid_data(line: usize, detail: impl Into<String>) -> DiscoveryError {
    DiscoveryError::InvalidLanguageData {
        line,
        detail: detail.into(),
    }
}
