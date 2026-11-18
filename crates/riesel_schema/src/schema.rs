use crate::{CapabilityRef, CapabilityRefError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDefinition {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSchema {
    pub version: u32,
    pub app: AppDefinition,
    pub models: BTreeMap<String, ModelDefinition>,
    #[serde(default)]
    pub requires: Vec<CapabilityRequirement>,
}

/// An application-level dependency name and its required capability version.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    pub name: String,
    pub capability: CapabilityRef,
    /// Requirement aliases that must be ready before this requirement starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Uuid,
    String,
    Bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefinition {
    pub fields: BTreeMap<String, FieldType>,
}

pub fn parse_schema(source: &str) -> Result<AppSchema, serde_json::Error> {
    serde_json::from_str(source)
}

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("unsupported schema version: {version}")]
    UnsupportedVersion { version: u32 },
    #[error("invalid capability requirement name: {name}")]
    InvalidRequirementName { name: String },
    #[error("duplicate capability requirement name: {name}")]
    DuplicateRequirementName { name: String },
    #[error("invalid capability for requirement {name}: {source}")]
    InvalidCapability {
        name: String,
        #[source]
        source: CapabilityRefError,
    },
    #[error("requirement {requirement} cannot depend on itself")]
    SelfDependency { requirement: String },
    #[error("requirement {requirement} depends on undeclared requirement {dependency}")]
    UnknownDependency { requirement: String, dependency: String },
    #[error("requirement {requirement} repeats dependency {dependency}")]
    DuplicateDependency { requirement: String, dependency: String },
    #[error("cyclic requirement dependencies prevent startup: {requirements:?}")]
    DependencyCycle { requirements: Vec<String> },
}

impl AppSchema {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.version != 1 {
            return Err(SchemaError::UnsupportedVersion {
                version: self.version,
            });
        }

        let mut names = BTreeSet::new();
        for requirement in &self.requires {
            if !valid_requirement_name(&requirement.name) {
                return Err(SchemaError::InvalidRequirementName {
                    name: requirement.name.clone(),
                });
            }
            if !names.insert(&requirement.name) {
                return Err(SchemaError::DuplicateRequirementName {
                    name: requirement.name.clone(),
                });
            }
            requirement
                .capability
                .validate()
                .map_err(|source| SchemaError::InvalidCapability {
                    name: requirement.name.clone(),
                    source,
                })?;
        }

        for requirement in &self.requires {
            let mut dependencies = BTreeSet::new();
            for dependency in &requirement.depends_on {
                if dependency == &requirement.name {
                    return Err(SchemaError::SelfDependency {
                        requirement: requirement.name.clone(),
                    });
                }
                if !names.contains(dependency.as_str()) {
                    return Err(SchemaError::UnknownDependency {
                        requirement: requirement.name.clone(),
                        dependency: dependency.clone(),
                    });
                }
                if !dependencies.insert(dependency) {
                    return Err(SchemaError::DuplicateDependency {
                        requirement: requirement.name.clone(),
                        dependency: dependency.clone(),
                    });
                }
            }
        }
        self.order_indices()?;
        Ok(())
    }

    /// Stable topological ordering: dependencies first, declaration order for
    /// independent requirements. No service is started during validation.
    pub fn startup_order(&self) -> Result<Vec<&CapabilityRequirement>, SchemaError> {
        self.validate()?;
        Ok(self
            .order_indices()?
            .into_iter()
            .map(|index| &self.requires[index])
            .collect())
    }

    fn order_indices(&self) -> Result<Vec<usize>, SchemaError> {
        let positions: BTreeMap<&str, usize> = self
            .requires
            .iter()
            .enumerate()
            .map(|(index, requirement)| (requirement.name.as_str(), index))
            .collect();
        let mut started = vec![false; self.requires.len()];
        let mut order = Vec::with_capacity(self.requires.len());
        while order.len() < self.requires.len() {
            let next = self.requires.iter().enumerate().find(|(index, requirement)| {
                !started[*index]
                    && requirement.depends_on.iter().all(|dependency| {
                        positions
                            .get(dependency.as_str())
                            .is_some_and(|position| started[*position])
                    })
            });
            let Some((index, _)) = next else {
                return Err(SchemaError::DependencyCycle {
                    requirements: self
                        .requires
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| !started[*index])
                        .map(|(_, requirement)| requirement.name.clone())
                        .collect(),
                });
            };
            started[index] = true;
            order.push(index);
        }
        Ok(order)
    }
}

fn valid_requirement_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[derive(Debug, Error)]
pub enum RecordError {
    #[error("record must be a JSON object")]
    ExpectedObject,

    #[error("missing required field: {field}")]
    MissingField { field: String },

    #[error("unknown field: {field}")]
    UnknownField { field: String },

    #[error("invalid value for field: {field}, expected: {expected}")]
    InvalidValue {
        field: String,
        expected: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_schema() {
        let source = r#"
        {
            "version": 1,
            "app": {
                "name": "Example"
            },
            "models": {
                "entry": {
                    "fields": {
                        "title": "string",
                        "active": "bool"
                    }
                }
            }
        }
        "#;

        let schema = parse_schema(source).unwrap();

        schema.validate().unwrap();

        assert_eq!(schema.app.name, "Example");

        let model = schema.models.get("entry").unwrap();

        assert!(matches!(model.fields.get("title"), Some(FieldType::String)));

        assert!(matches!(model.fields.get("active"), Some(FieldType::Bool)));
    }

    #[test]
    fn rejects_unsupported_version() {
        let source = r#"
        {
            "version": 2,
            "app": {
                "name": "Example"
            },
            "models": {}
        }
        "#;

        let schema = parse_schema(source).unwrap();

        assert!(matches!(
            schema.validate(),
            Err(SchemaError::UnsupportedVersion { version: 2 })
        ));
    }

    #[test]
    fn parses_and_validates_capability_requirements() {
        let source = r#"{
            "version": 1,
            "app": {"name": "Todo"},
            "models": {},
            "requires": [
                {"name": "storage", "capability": {"id": "riesel.storage.records", "version": 1}}
            ]
        }"#;
        let schema = parse_schema(source).unwrap();
        schema.validate().unwrap();
        assert_eq!(schema.requires.len(), 1);
        assert_eq!(schema.requires[0].name, "storage");
    }

    #[test]
    fn detects_duplicate_requirement_names() {
        let source = r#"{
            "version": 1,
            "app": {"name": "Todo"},
            "models": {},
            "requires": [
                {"name": "storage", "capability": {"id": "riesel.storage.records", "version": 1}},
                {"name": "storage", "capability": {"id": "riesel.storage.records", "version": 2}}
            ]
        }"#;
        let schema = parse_schema(source).unwrap();
        assert!(matches!(
            schema.validate(),
            Err(SchemaError::DuplicateRequirementName { name }) if name == "storage"
        ));
    }

    #[test]
    fn rejects_invalid_capability_requirements() {
        let source = r#"{
            "version": 1,
            "app": {"name": "Todo"},
            "models": {},
            "requires": [
                {"name": "storage", "capability": {"id": "riesel.storage.records", "version": 0}}
            ]
        }"#;
        let schema = parse_schema(source).unwrap();
        assert!(matches!(
            schema.validate(),
            Err(SchemaError::InvalidCapability {
                source: CapabilityRefError::InvalidVersion,
                ..
            })
        ));
    }

    #[test]
    fn rejects_invalid_requirement_names() {
        let source = r#"{
            "version": 1,
            "app": {"name": "Todo"},
            "models": {},
            "requires": [
                {"name": "Storage API", "capability": {"id": "riesel.storage.records", "version": 1}}
            ]
        }"#;
        let schema = parse_schema(source).unwrap();
        assert!(matches!(
            schema.validate(),
            Err(SchemaError::InvalidRequirementName { .. })
        ));
    }

    #[test]
    fn starts_dependencies_before_consumers_with_stable_independent_order() {
        let schema = parse_schema(r#"{
            "version": 1, "app": {"name":"Example"}, "models": {},
            "requires": [
                {"name":"frontend", "capability":{"id":"example.frontend","version":1},
                 "depends_on":["worker","storage"]},
                {"name":"worker", "capability":{"id":"example.worker","version":1},
                 "depends_on":["storage"]},
                {"name":"independent", "capability":{"id":"example.other","version":1}},
                {"name":"storage", "capability":{"id":"example.storage","version":1}}
            ]
        }"#).unwrap();
        let names: Vec<_> = schema.startup_order().unwrap().iter()
            .map(|requirement| requirement.name.as_str()).collect();
        assert_eq!(names, ["independent", "storage", "worker", "frontend"]);
        assert!(!serde_json::to_string(&schema.requires[2]).unwrap().contains("depends_on"));
    }

    #[test]
    fn rejects_missing_self_duplicate_and_cyclic_dependencies() {
        let schema_with = |depends_on: Vec<&str>, other: Vec<&str>| {
            let source = serde_json::json!({
                "version": 1, "app": {"name": "Example"}, "models": {},
                "requires": [
                    {"name":"one", "capability":{"id":"example.first","version":1},
                     "depends_on": depends_on},
                    {"name":"two", "capability":{"id":"example.second","version":1},
                     "depends_on": other}
                ]
            });
            parse_schema(&source.to_string()).unwrap()
        };
        assert!(matches!(
            schema_with(vec!["unknown"], vec![]).validate(),
            Err(SchemaError::UnknownDependency { .. })
        ));
        assert!(matches!(
            schema_with(vec!["one"], vec![]).validate(),
            Err(SchemaError::SelfDependency { .. })
        ));
        assert!(matches!(
            schema_with(vec!["two", "two"], vec![]).validate(),
            Err(SchemaError::DuplicateDependency { .. })
        ));
        assert!(matches!(
            schema_with(vec!["two"], vec!["one"]).validate(),
            Err(SchemaError::DependencyCycle { .. })
        ));
    }
}
