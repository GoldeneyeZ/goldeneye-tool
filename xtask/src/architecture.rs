//! Enforces workspace dependency direction during the layered migration.

use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

const LAYER_ORDER: [&str; 5] = ["domain", "ports", "application", "adapters", "delivery"];

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct ArchitectureReport {
    pub packages: usize,
    pub dependencies: usize,
    pub exceptions: usize,
}

#[derive(Debug, Error)]
pub enum ArchitectureError {
    #[error("failed to run cargo metadata: {0}")]
    MetadataProcess(#[source] std::io::Error),
    #[error("cargo metadata failed: {0}")]
    MetadataCommand(String),
    #[error("invalid cargo metadata: {0}")]
    MetadataJson(#[from] serde_json::Error),
    #[error("invalid workspace architecture metadata: {0}")]
    InvalidConfiguration(String),
    #[error("architecture violations:\n{0}")]
    Violations(String),
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    #[serde(rename = "metadata")]
    workspace_metadata: Value,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceArchitecture {
    layers: Vec<String>,
    #[serde(default)]
    excluded: BTreeSet<String>,
    #[serde(default)]
    exceptions: BTreeSet<String>,
    packages: BTreeMap<String, String>,
}

/// Verifies current workspace against workspace architecture metadata.
///
/// # Errors
///
/// Returns configuration, metadata, or dependency-direction violations.
pub fn verify_architecture() -> Result<ArchitectureReport, ArchitectureError> {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .map_err(ArchitectureError::MetadataProcess)?;
    if !output.status.success() {
        return Err(ArchitectureError::MetadataCommand(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let metadata = serde_json::from_slice::<CargoMetadata>(&output.stdout)?;
    verify_metadata(&metadata)
}

fn verify_metadata(metadata: &CargoMetadata) -> Result<ArchitectureReport, ArchitectureError> {
    let architecture = serde_json::from_value::<WorkspaceArchitecture>(
        metadata
            .workspace_metadata
            .get("architecture")
            .cloned()
            .ok_or_else(|| {
                ArchitectureError::InvalidConfiguration(
                    "missing workspace.metadata.architecture".to_owned(),
                )
            })?,
    )
    .map_err(|error| ArchitectureError::InvalidConfiguration(error.to_string()))?;
    validate_configuration(&architecture)?;

    let members = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .collect::<Vec<_>>();
    let member_names = members
        .iter()
        .map(|package| package.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut violations = Vec::new();

    for package in &members {
        if architecture.excluded.contains(&package.name) {
            continue;
        }
        if !architecture.packages.contains_key(&package.name) {
            violations.push(format!("unclassified workspace package {}", package.name));
        }
    }
    for package in architecture.packages.keys() {
        if !member_names.contains(package.as_str()) {
            violations.push(format!(
                "classified package {package} is not a workspace member"
            ));
        }
    }

    let mut dependencies = 0;
    let mut used_exceptions = BTreeSet::new();
    for package in members {
        let Some(source_layer) = architecture.packages.get(&package.name) else {
            continue;
        };
        for dependency in &package.dependencies {
            if dependency.kind.as_deref() == Some("dev")
                || !architecture.packages.contains_key(&dependency.name)
            {
                continue;
            }
            dependencies += 1;
            let target_layer = &architecture.packages[&dependency.name];
            if dependency_allowed(source_layer, target_layer) {
                continue;
            }
            let edge = format!("{}->{}", package.name, dependency.name);
            if architecture.exceptions.contains(&edge) {
                used_exceptions.insert(edge);
            } else {
                violations.push(format!(
                    "{edge}: forbidden {source_layer} -> {target_layer} dependency"
                ));
            }
        }
    }

    for exception in architecture.exceptions.difference(&used_exceptions) {
        violations.push(format!("stale or allowed migration exception {exception}"));
    }
    if !violations.is_empty() {
        return Err(ArchitectureError::Violations(violations.join("\n")));
    }

    Ok(ArchitectureReport {
        packages: architecture.packages.len(),
        dependencies,
        exceptions: used_exceptions.len(),
    })
}

fn validate_configuration(architecture: &WorkspaceArchitecture) -> Result<(), ArchitectureError> {
    if architecture.layers != LAYER_ORDER {
        return Err(ArchitectureError::InvalidConfiguration(format!(
            "layers must be {LAYER_ORDER:?}"
        )));
    }
    if architecture
        .packages
        .values()
        .any(|layer| !LAYER_ORDER.contains(&layer.as_str()))
    {
        return Err(ArchitectureError::InvalidConfiguration(
            "package references an unknown layer".to_owned(),
        ));
    }
    Ok(())
}

fn dependency_allowed(source: &str, target: &str) -> bool {
    match source {
        "domain" => target == "domain",
        "ports" => matches!(target, "domain" | "ports"),
        "application" => matches!(target, "domain" | "ports" | "application"),
        "adapters" => matches!(target, "domain" | "ports" | "adapters"),
        "delivery" => LAYER_ORDER.contains(&target),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        ArchitectureError, CargoMetadata, Dependency, Package, dependency_allowed, verify_metadata,
    };

    #[test]
    fn inward_dependencies_follow_clean_architecture_direction() {
        assert!(dependency_allowed("domain", "domain"));
        assert!(dependency_allowed("ports", "domain"));
        assert!(dependency_allowed("application", "ports"));
        assert!(dependency_allowed("adapters", "ports"));
        assert!(dependency_allowed("delivery", "application"));

        assert!(!dependency_allowed("domain", "ports"));
        assert!(!dependency_allowed("application", "adapters"));
        assert!(!dependency_allowed("adapters", "application"));
    }

    #[test]
    fn verifier_rejects_unapproved_outward_dependency() {
        let error = verify_metadata(&fixture(&[])).expect_err("dependency must be rejected");

        assert!(matches!(error, ArchitectureError::Violations(_)), "{error}");
        assert!(
            error
                .to_string()
                .contains("app->database: forbidden application -> adapters dependency"),
            "{error}"
        );
    }

    #[test]
    fn verifier_accepts_only_exact_migration_exception() {
        let report = verify_metadata(&fixture(&["app->database"])).expect("exception");

        assert_eq!(report.packages, 3);
        assert_eq!(report.dependencies, 1);
        assert_eq!(report.exceptions, 1);

        let error = verify_metadata(&fixture(&["app->database", "app->missing"]))
            .expect_err("stale exception must fail");
        assert!(
            error
                .to_string()
                .contains("stale or allowed migration exception app->missing"),
            "{error}"
        );
    }

    fn fixture(exceptions: &[&str]) -> CargoMetadata {
        CargoMetadata {
            packages: vec![
                Package {
                    id: "domain".to_owned(),
                    name: "domain".to_owned(),
                    dependencies: Vec::new(),
                },
                Package {
                    id: "app".to_owned(),
                    name: "app".to_owned(),
                    dependencies: vec![Dependency {
                        name: "database".to_owned(),
                        kind: None,
                    }],
                },
                Package {
                    id: "database".to_owned(),
                    name: "database".to_owned(),
                    dependencies: Vec::new(),
                },
            ],
            workspace_members: vec!["domain".to_owned(), "app".to_owned(), "database".to_owned()],
            workspace_metadata: json!({
                "architecture": {
                    "layers": ["domain", "ports", "application", "adapters", "delivery"],
                    "packages": {
                        "domain": "domain",
                        "app": "application",
                        "database": "adapters"
                    },
                    "exceptions": exceptions,
                    "excluded": []
                }
            }),
        }
    }
}
