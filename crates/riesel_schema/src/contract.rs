use crate::CapabilityRef;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Inspectable description of a capability's public interface.
///
/// This is metadata, not an executable implementation or a wire protocol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityContract {
    pub capability: CapabilityRef,
    pub operations: BTreeMap<String, OperationDefinition>,

    #[serde(default)]
    pub types: BTreeMap<String, TypeDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Command,
    Query,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationDefinition {
    pub kind: OperationKind,
    pub description: String,

    #[serde(default)]
    pub inputs: BTreeMap<String, TypeRef>,

    pub output: TypeRef,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<TypeRef>,
}

/// Platform-visible data types, independent of Rust borrowing and transport.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeRef {
    Unit,
    String,
    Bool,
    Uuid,
    U64,
    Json,
    Named { name: String },
    Optional { item: Box<TypeRef> },
    List { item: Box<TypeRef> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDefinition {
    Struct {
        fields: BTreeMap<String, TypeRef>,
    },
    Enum {
        /// An absent payload denotes a unit variant.
        variants: BTreeMap<String, Option<TypeRef>>,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("invalid capability identifier: {0}")]
    InvalidCapabilityId(String),

    #[error("capability version must be greater than zero")]
    InvalidVersion,

    #[error("capability must expose at least one operation")]
    NoOperations,

    #[error("invalid {kind} name: {name}")]
    InvalidName { kind: &'static str, name: String },

    #[error("operation has no description: {0}")]
    MissingDescription(String),

    #[error("type has no variants or fields: {0}")]
    EmptyType(String),

    #[error("reference to undeclared type: {0}")]
    UnknownType(String),
}

impl CapabilityContract {
    pub fn validate(&self) -> Result<(), ContractError> {
        if !valid_capability_id(&self.capability.id) {
            return Err(ContractError::InvalidCapabilityId(
                self.capability.id.clone(),
            ));
        }
        if self.capability.version == 0 {
            return Err(ContractError::InvalidVersion);
        }
        if self.operations.is_empty() {
            return Err(ContractError::NoOperations);
        }

        for (name, definition) in &self.types {
            validate_name("type", name)?;
            match definition {
                TypeDefinition::Struct { fields } => {
                    if fields.is_empty() {
                        return Err(ContractError::EmptyType(name.clone()));
                    }
                    for (field, kind) in fields {
                        validate_name("field", field)?;
                        self.validate_type(kind)?;
                    }
                }
                TypeDefinition::Enum { variants } => {
                    if variants.is_empty() {
                        return Err(ContractError::EmptyType(name.clone()));
                    }
                    for (variant, payload) in variants {
                        validate_name("variant", variant)?;
                        if let Some(payload) = payload {
                            self.validate_type(payload)?;
                        }
                    }
                }
            }
        }

        for (name, operation) in &self.operations {
            validate_name("operation", name)?;
            if operation.description.trim().is_empty() {
                return Err(ContractError::MissingDescription(name.clone()));
            }
            for (parameter, kind) in &operation.inputs {
                validate_name("parameter", parameter)?;
                self.validate_type(kind)?;
            }
            self.validate_type(&operation.output)?;
            if let Some(error) = &operation.error {
                self.validate_type(error)?;
            }
        }
        Ok(())
    }

    fn validate_type(&self, kind: &TypeRef) -> Result<(), ContractError> {
        match kind {
            TypeRef::Named { name } => {
                if !self.types.contains_key(name) {
                    return Err(ContractError::UnknownType(name.clone()));
                }
            }
            TypeRef::Optional { item } | TypeRef::List { item } => self.validate_type(item)?,
            _ => {}
        }
        Ok(())
    }
}

fn validate_name(kind: &'static str, name: &str) -> Result<(), ContractError> {
    let mut chars = name.chars();
    if !matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(ContractError::InvalidName {
            kind,
            name: name.to_owned(),
        });
    }
    Ok(())
}

fn valid_capability_id(id: &str) -> bool {
    let segments: Vec<_> = id.split('.').collect();
    segments.len() >= 2
        && segments.iter().all(|part| {
            let mut chars = part.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example() -> CapabilityContract {
        CapabilityContract {
            capability: CapabilityRef {
                id: "example.records".into(),
                version: 1,
            },
            operations: BTreeMap::from([(
                "get".into(),
                OperationDefinition {
                    kind: OperationKind::Query,
                    description: "Gets one record".into(),
                    inputs: BTreeMap::from([("id".into(), TypeRef::Uuid)]),
                    output: TypeRef::Optional {
                        item: Box::new(TypeRef::Named {
                            name: "Record".into(),
                        }),
                    },
                    error: None,
                },
            )]),
            types: BTreeMap::from([(
                "Record".into(),
                TypeDefinition::Struct {
                    fields: BTreeMap::from([("id".into(), TypeRef::Uuid)]),
                },
            )]),
        }
    }

    #[test]
    fn valid_contract_round_trips_through_json() {
        let contract = example();
        contract.validate().unwrap();

        let encoded = serde_json::to_string(&contract).unwrap();
        let decoded: CapabilityContract = serde_json::from_str(&encoded).unwrap();
        assert_eq!(contract, decoded);
        decoded.validate().unwrap();
    }

    #[test]
    fn rejects_unknown_referenced_types() {
        let mut contract = example();
        contract.types.clear();
        assert_eq!(
            contract.validate(),
            Err(ContractError::UnknownType("Record".into()))
        );
    }

    #[test]
    fn rejects_invalid_identity_and_version() {
        let mut contract = example();
        contract.capability.id = "Not Valid".into();
        assert!(matches!(
            contract.validate(),
            Err(ContractError::InvalidCapabilityId(_))
        ));

        contract.capability.id = "example.records".into();
        contract.capability.version = 0;
        assert_eq!(contract.validate(), Err(ContractError::InvalidVersion));
    }

    #[test]
    fn rejects_empty_operations_and_descriptions() {
        let mut contract = example();
        contract.operations.get_mut("get").unwrap().description.clear();
        assert_eq!(
            contract.validate(),
            Err(ContractError::MissingDescription("get".into()))
        );

        contract.operations.clear();
        assert_eq!(contract.validate(), Err(ContractError::NoOperations));
    }
}
