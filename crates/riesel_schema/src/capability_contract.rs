use crate::{CapabilityRef, CapabilityRefError};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// A capability API derived from its Rust trait and public data types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityContract {
    pub capability: CapabilityRef,
    pub operations: BTreeMap<String, OperationDefinition>,
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
    /// Ordered, to preserve the callable Rust signature.
    pub inputs: Vec<ParameterDefinition>,
    pub output: TypeRef,
    pub error: TypeRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterDefinition {
    pub name: String,
    pub kind: TypeRef,
}

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
    /// None is a unit variant; Some is a single-field tuple variant.
    Enum {
        variants: BTreeMap<String, Option<TypeRef>>,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ContractError {
    #[error("invalid capability identifier: {0}")]
    InvalidCapabilityId(String),
    #[error("capability version must be greater than zero")]
    InvalidVersion,
    #[error("a capability must expose at least one operation")]
    NoOperations,
    #[error("invalid {kind} name: {name}")]
    InvalidName { kind: &'static str, name: String },
    #[error("operation {0} must have a description")]
    MissingDescription(String),
    #[error("duplicate parameter {parameter} in operation {operation}")]
    DuplicateParameter {
        operation: String,
        parameter: String,
    },
    #[error("reference to undeclared type: {0}")]
    UnknownType(String),
    #[error("different Rust types claim the same contract type name: {0}")]
    ConflictingTypeName(String),
}

impl CapabilityContract {
    /// Compare declared API shape without treating documentation changes as
    /// contract-breaking. This is not proof of provider conformance or security.
    pub fn has_same_api_shape(&self, other: &Self) -> bool {
        self.capability == other.capability
            && self.types == other.types
            && self.operations.len() == other.operations.len()
            && self.operations.iter().all(|(name, left)| {
                other.operations.get(name).is_some_and(|right| {
                    left.kind == right.kind
                        && left.inputs == right.inputs
                        && left.output == right.output
                        && left.error == right.error
                })
            })
    }

    pub fn validate(&self) -> Result<(), ContractError> {
        self.capability.validate().map_err(|error| match error {
            CapabilityRefError::InvalidId(id) => ContractError::InvalidCapabilityId(id),
            CapabilityRefError::InvalidVersion => ContractError::InvalidVersion,
        })?;
        if self.operations.is_empty() {
            return Err(ContractError::NoOperations);
        }
        for (name, definition) in &self.types {
            validate_name("type", name)?;
            match definition {
                TypeDefinition::Struct { fields } => {
                    for (field, kind) in fields {
                        validate_name("field", field)?;
                        self.validate_type(kind)?;
                    }
                }
                TypeDefinition::Enum { variants } => {
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
            let mut seen = BTreeSet::new();
            for input in &operation.inputs {
                validate_name("parameter", &input.name)?;
                if !seen.insert(&input.name) {
                    return Err(ContractError::DuplicateParameter {
                        operation: name.clone(),
                        parameter: input.name.clone(),
                    });
                }
                self.validate_type(&input.kind)?;
            }
            self.validate_type(&operation.output)?;
            self.validate_type(&operation.error)?;
        }
        Ok(())
    }

    fn validate_type(&self, kind: &TypeRef) -> Result<(), ContractError> {
        match kind {
            TypeRef::Named { name } if !self.types.contains_key(name) => {
                Err(ContractError::UnknownType(name.clone()))
            }
            TypeRef::Optional { item } | TypeRef::List { item } => self.validate_type(item),
            _ => Ok(()),
        }
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

/// Describes serializable public API types. Derive this for named structs/enums;
/// primitives and Option/Vec have built-in implementations.
pub trait RieselType {
    fn type_ref() -> TypeRef;

    fn collect_definitions(
        _types: &mut BTreeMap<String, TypeDefinition>,
    ) -> Result<(), ContractError> {
        Ok(())
    }
}

macro_rules! primitive {
    ($ty:ty, $variant:ident) => {
        impl RieselType for $ty {
            fn type_ref() -> TypeRef {
                TypeRef::$variant
            }
        }
    };
}

primitive!((), Unit);
primitive!(String, String);
primitive!(&str, String);
primitive!(bool, Bool);
primitive!(u64, U64);
primitive!(uuid::Uuid, Uuid);
primitive!(serde_json::Value, Json);

impl<T: RieselType> RieselType for Option<T> {
    fn type_ref() -> TypeRef {
        TypeRef::Optional {
            item: Box::new(T::type_ref()),
        }
    }

    fn collect_definitions(
        types: &mut BTreeMap<String, TypeDefinition>,
    ) -> Result<(), ContractError> {
        T::collect_definitions(types)
    }
}

impl<T: RieselType> RieselType for Vec<T> {
    fn type_ref() -> TypeRef {
        TypeRef::List {
            item: Box::new(T::type_ref()),
        }
    }

    fn collect_definitions(
        types: &mut BTreeMap<String, TypeDefinition>,
    ) -> Result<(), ContractError> {
        T::collect_definitions(types)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitive_and_container_types_are_inspectable() {
        assert_eq!(<&str as RieselType>::type_ref(), TypeRef::String);
        assert_eq!(
            <Option<Vec<uuid::Uuid>> as RieselType>::type_ref(),
            TypeRef::Optional {
                item: Box::new(TypeRef::List {
                    item: Box::new(TypeRef::Uuid),
                }),
            }
        );
    }

    #[test]
    fn an_unknown_named_type_is_rejected() {
        let contract = CapabilityContract {
            capability: CapabilityRef {
                id: "example.records".into(),
                version: 1,
            },
            operations: BTreeMap::from([(
                "read".into(),
                OperationDefinition {
                    kind: OperationKind::Query,
                    description: "Get one record".into(),
                    inputs: vec![],
                    output: TypeRef::Named {
                        name: "Missing".into(),
                    },
                    error: TypeRef::Unit,
                },
            )]),
            types: BTreeMap::new(),
        };

        assert_eq!(
            contract.validate(),
            Err(ContractError::UnknownType("Missing".into()))
        );
    }
    #[test]
    fn contract_signature_comparison_ignores_docs_but_not_api_types() {
        let mut contract = CapabilityContract {
            capability: CapabilityRef {
                id: "example.records".into(),
                version: 1,
            },
            operations: BTreeMap::from([(
                "read".into(),
                OperationDefinition {
                    kind: OperationKind::Query,
                    description: "Read records".into(),
                    inputs: vec![ParameterDefinition {
                        name: "id".into(),
                        kind: TypeRef::Uuid,
                    }],
                    output: TypeRef::Json,
                    error: TypeRef::Unit,
                },
            )]),
            types: BTreeMap::new(),
        };
        let original = contract.clone();
        contract.operations.get_mut("read").unwrap().description = "Retrieve data".into();
        assert!(contract.has_same_api_shape(&original));

        contract.operations.get_mut("read").unwrap().inputs[0].kind = TypeRef::String;
        assert!(!contract.has_same_api_shape(&original));

        contract.operations.get_mut("read").unwrap().inputs[0].kind = TypeRef::Uuid;
        contract.capability.version = 2;
        assert!(!contract.has_same_api_shape(&original));
    }
}
