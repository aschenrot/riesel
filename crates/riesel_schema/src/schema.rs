use crate::{CapabilityRef, CapabilityRefError};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDefinition {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSchema {
    pub version: u32,
    pub app: AppDefinition,
    pub models: BTreeMap<String, ModelDefinition>,
    #[serde(default)]
    pub requires: Vec<CapabilityRequirement>,
}

/// An application-level dependency name and its required capability version.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    pub name: String,
    pub capability: CapabilityRef,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Uuid,
    String,
    Bool,
}

#[derive(Debug, Clone, Deserialize)]
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

        Ok(())
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
}
