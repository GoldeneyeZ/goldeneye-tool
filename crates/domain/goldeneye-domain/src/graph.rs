use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{ContentHash, Generation, ProjectId, ProjectRelativePath, SourceSpan};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum GraphIdentityError {
    #[error("{kind} must not be empty")]
    Empty { kind: &'static str },
    #[error("{kind} must not contain NUL")]
    Nul { kind: &'static str },
    #[error("graph node name must not be empty")]
    EmptyNodeName,
    #[error("graph node name must not contain NUL")]
    NulNodeName,
    #[error("project root path must not be empty")]
    EmptyProjectRoot,
    #[error("project root path must not contain NUL")]
    NulProjectRoot,
}

fn validate_identifier(value: &str, kind: &'static str) -> Result<(), GraphIdentityError> {
    if value.is_empty() {
        return Err(GraphIdentityError::Empty { kind });
    }
    if value.contains('\0') {
        return Err(GraphIdentityError::Nul { kind });
    }
    Ok(())
}

macro_rules! validated_string_id {
    ($name:ident, $kind:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Creates a validated identifier.
            ///
            /// # Errors
            ///
            /// Returns an error when the value is empty or contains NUL.
            pub fn new(value: impl Into<String>) -> Result<Self, GraphIdentityError> {
                let value = value.into();
                validate_identifier(&value, $kind)?;
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
            }
        }
    };
}

validated_string_id!(NodeId, "node ID");
validated_string_id!(NodeLabel, "node label");
validated_string_id!(QualifiedName, "qualified name");
validated_string_id!(EdgeKind, "edge kind");

#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeDiscriminator(String);

impl EdgeDiscriminator {
    /// Creates an optional edge discriminator.
    ///
    /// # Errors
    ///
    /// Returns an error when the value contains NUL.
    pub fn new(value: impl Into<String>) -> Result<Self, GraphIdentityError> {
        let value = value.into();
        if value.contains('\0') {
            return Err(GraphIdentityError::Nul {
                kind: "edge discriminator",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for EdgeDiscriminator {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EdgeDiscriminator {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::new(String::deserialize(deserializer)?).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub root_path: String,
    pub generation: Generation,
}

impl ProjectRecord {
    /// Creates a project registry record at generation zero.
    ///
    /// # Errors
    ///
    /// Returns an error when `root_path` is empty or contains NUL.
    pub fn new(id: ProjectId, root_path: impl Into<String>) -> Result<Self, GraphIdentityError> {
        let root_path = root_path.into();
        if root_path.is_empty() {
            return Err(GraphIdentityError::EmptyProjectRoot);
        }
        if root_path.contains('\0') {
            return Err(GraphIdentityError::NulProjectRoot);
        }
        Ok(Self {
            id,
            root_path,
            generation: Generation::new(0),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct FileId {
    pub project: ProjectId,
    pub path: ProjectRelativePath,
}

impl FileId {
    #[must_use]
    pub const fn new(project: ProjectId, path: ProjectRelativePath) -> Self {
        Self { project, path }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub id: FileId,
    pub content_hash: ContentHash,
    pub generation: Generation,
    pub modified_ns: u64,
    pub byte_len: u64,
}

impl FileRecord {
    #[must_use]
    pub const fn new(
        id: FileId,
        content_hash: ContentHash,
        generation: Generation,
        modified_ns: u64,
        byte_len: u64,
    ) -> Self {
        Self {
            id,
            content_hash,
            generation,
            modified_ns,
            byte_len,
        }
    }
}

pub type GraphProperties = BTreeMap<String, Value>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub project: ProjectId,
    pub id: NodeId,
    pub label: NodeLabel,
    pub name: String,
    pub qualified_name: QualifiedName,
    pub file_path: Option<ProjectRelativePath>,
    pub source_span: Option<SourceSpan>,
    pub generation: Generation,
    pub properties: GraphProperties,
}

impl GraphNode {
    /// Creates a graph node with empty properties.
    ///
    /// # Errors
    ///
    /// Returns an error when `name` is empty or contains NUL.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project: ProjectId,
        id: NodeId,
        label: NodeLabel,
        name: impl Into<String>,
        qualified_name: QualifiedName,
        file_path: Option<ProjectRelativePath>,
        source_span: Option<SourceSpan>,
        generation: Generation,
    ) -> Result<Self, GraphIdentityError> {
        let name = name.into();
        if name.is_empty() {
            return Err(GraphIdentityError::EmptyNodeName);
        }
        if name.contains('\0') {
            return Err(GraphIdentityError::NulNodeName);
        }
        Ok(Self {
            project,
            id,
            label,
            name,
            qualified_name,
            file_path,
            source_span,
            generation,
            properties: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn with_properties(mut self, properties: GraphProperties) -> Self {
        self.properties = properties;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub project: ProjectId,
    pub source: NodeId,
    pub target: NodeId,
    pub kind: EdgeKind,
    pub discriminator: EdgeDiscriminator,
    pub generation: Generation,
    pub properties: GraphProperties,
}

impl GraphEdge {
    #[must_use]
    pub fn new(
        project: ProjectId,
        source: NodeId,
        target: NodeId,
        kind: EdgeKind,
        generation: Generation,
    ) -> Self {
        Self {
            project,
            source,
            target,
            kind,
            discriminator: EdgeDiscriminator::default(),
            generation,
            properties: BTreeMap::new(),
        }
    }

    /// Adds a stable discriminator to the edge identity.
    ///
    /// # Errors
    ///
    /// Returns an error when `value` contains NUL.
    pub fn with_discriminator(
        mut self,
        value: impl Into<String>,
    ) -> Result<Self, GraphIdentityError> {
        self.discriminator = EdgeDiscriminator::new(value)?;
        Ok(self)
    }

    #[must_use]
    pub fn with_properties(mut self, properties: GraphProperties) -> Self {
        self.properties = properties;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{EdgeDiscriminator, GraphIdentityError, NodeId, ProjectRecord};
    use crate::ProjectId;

    #[test]
    fn identifiers_reject_empty_and_nul_values() {
        assert_eq!(
            NodeId::new(""),
            Err(GraphIdentityError::Empty { kind: "node ID" })
        );
        assert_eq!(
            EdgeDiscriminator::new("bad\0value"),
            Err(GraphIdentityError::Nul {
                kind: "edge discriminator"
            })
        );
    }

    #[test]
    fn project_record_starts_at_generation_zero() {
        let record =
            ProjectRecord::new(ProjectId::new("p").expect("ID"), "/repo").expect("project record");
        assert_eq!(record.generation.value(), 0);
    }
}
