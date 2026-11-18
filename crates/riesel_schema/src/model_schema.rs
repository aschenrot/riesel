use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Portable record field types. Unit variants retain their original string
/// serialization; containers use externally tagged recursive JSON shapes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Uuid,
    String,
    Bool,
    U64,
    I64,
    F64,
    Json,
    Optional(Box<FieldType>),
    List(Box<FieldType>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelDefinition {
    pub fields: BTreeMap<String, FieldType>,
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
