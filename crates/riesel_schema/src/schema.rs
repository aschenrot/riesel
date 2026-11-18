use crate::{CapabilityRef, CapabilityRefError, ModelDefinition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppDefinition {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppSchema {
    pub version: u32,
    pub app: AppDefinition,
    pub models: BTreeMap<String, ModelDefinition>,
    /// Model names mapped to the declared capability requirement aliases
    /// selected for their record storage. An empty map denotes a schema
    /// without explicit model-to-provider selection.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub model_bindings: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub requires: Vec<CapabilityRequirement>,
}

/// An application-level dependency name and its required capability version.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequirement {
    pub name: String,
    pub capability: CapabilityRef,
    /// Requirement aliases that must be ready before this requirement starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
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
    UnknownDependency {
        requirement: String,
        dependency: String,
    },
    #[error("requirement {requirement} repeats dependency {dependency}")]
    DuplicateDependency {
        requirement: String,
        dependency: String,
    },
    #[error("cyclic requirement dependencies prevent startup: {requirements:?}")]
    DependencyCycle { requirements: Vec<String> },
    #[error("model binding refers to undeclared model {model}")]
    UnknownBoundModel { model: String },
    #[error("model {model} binds undeclared requirement {requirement}")]
    UnknownModelBinding { model: String, requirement: String },
    #[error("model {model} binds requirement {requirement} more than once")]
    DuplicateModelBinding { model: String, requirement: String },
    #[error("invalid model identifier: {0}")]
    InvalidModelName(String),
    #[error("model {0} must declare at least one field")]
    EmptyModel(String),
    #[error("model {model} contains invalid field identifier: {field}")]
    InvalidModelField { model: String, field: String },
    #[error("model {model} contains unsupported field type: {field}")]
    UnsupportedModelType { model: String, field: String },
}

impl AppSchema {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.version != 1 {
            return Err(SchemaError::UnsupportedVersion {
                version: self.version,
            });
        }

        for (name, model) in &self.models {
            if !valid_requirement_name(name) {
                return Err(SchemaError::InvalidModelName(name.clone()));
            }
            if model.fields.is_empty() {
                return Err(SchemaError::EmptyModel(name.clone()));
            }
            for (field, kind) in &model.fields {
                if !valid_model_field_name(field) {
                    return Err(SchemaError::InvalidModelField {
                        model: name.clone(),
                        field: field.clone(),
                    });
                }
                if !valid_model_type(kind) {
                    return Err(SchemaError::UnsupportedModelType {
                        model: name.clone(),
                        field: field.clone(),
                    });
                }
            }
        }

        let mut names = BTreeSet::new();
        for requirement in &self.requires {
            if !valid_requirement_name(&requirement.name) {
                return Err(SchemaError::InvalidRequirementName {
                    name: requirement.name.clone(),
                });
            }
            if !names.insert(requirement.name.as_str()) {
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

        for (model, bindings) in &self.model_bindings {
            if !self.models.contains_key(model) {
                return Err(SchemaError::UnknownBoundModel {
                    model: model.clone(),
                });
            }
            let mut seen = BTreeSet::new();
            for requirement in bindings {
                if !names.contains(requirement.as_str()) {
                    return Err(SchemaError::UnknownModelBinding {
                        model: model.clone(),
                        requirement: requirement.clone(),
                    });
                }
                if !seen.insert(requirement) {
                    return Err(SchemaError::DuplicateModelBinding {
                        model: model.clone(),
                        requirement: requirement.clone(),
                    });
                }
            }
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
}

fn valid_model_type(kind: &crate::TypeRef) -> bool {
    match kind {
        crate::TypeRef::Unit | crate::TypeRef::Named { .. } => false,
        crate::TypeRef::Optional { item } | crate::TypeRef::List { item } => valid_model_type(item),
        _ => true,
    }
}

fn valid_requirement_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn valid_model_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FieldType;

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
                        "title": {"kind": "string"},
                        "active": {"kind": "bool"}
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
        let schema = parse_schema(
            r#"{
            "version": 1, "app": {"name":"Example"}, "models": {},
            "requires": [
                {"name":"frontend", "capability":{"id":"example.frontend","version":1},
                 "depends_on":["worker","storage"]},
                {"name":"worker", "capability":{"id":"example.worker","version":1},
                 "depends_on":["storage"]},
                {"name":"independent", "capability":{"id":"example.other","version":1}},
                {"name":"storage", "capability":{"id":"example.storage","version":1}}
            ]
        }"#,
        )
        .unwrap();
        let names: Vec<_> = schema
            .startup_order()
            .unwrap()
            .iter()
            .map(|requirement| requirement.name.as_str())
            .collect();
        assert_eq!(names, ["independent", "storage", "worker", "frontend"]);
        assert!(
            !serde_json::to_string(&schema.requires[2])
                .unwrap()
                .contains("depends_on")
        );
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
    #[test]
    fn rejects_model_bindings_for_undeclared_models_or_requirements() {
        let mut schema = parse_schema(
            r#"{"version":1,"app":{"name":"Models"},
               "models":{"task":{"fields":{"title":{"kind":"string"}}}},
               "requires":[{"name":"storage","capability":{
                 "id":"riesel.storage.records","version":2}}]}"#,
        )
        .unwrap();
        schema
            .model_bindings
            .insert("task".into(), vec!["storage".into()]);
        schema.validate().unwrap();

        schema
            .model_bindings
            .insert("unknown".into(), vec!["storage".into()]);
        assert!(matches!(
            schema.validate(),
            Err(SchemaError::UnknownBoundModel { model }) if model == "unknown"
        ));
        schema.model_bindings.remove("unknown");

        schema
            .model_bindings
            .insert("task".into(), vec!["missing".into()]);
        assert!(matches!(
            schema.validate(),
            Err(SchemaError::UnknownModelBinding { requirement, .. })
                if requirement == "missing"
        ));

        schema
            .model_bindings
            .insert("task".into(), vec!["storage".into(), "storage".into()]);
        assert!(matches!(
            schema.validate(),
            Err(SchemaError::DuplicateModelBinding { requirement, .. })
                if requirement == "storage"
        ));
    }
}
