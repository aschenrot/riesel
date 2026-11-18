use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Model fields and capability signatures share one structural type algebra.
/// The schema now serializes every field using TypeRef's tagged representation,
/// avoiding contradictory interpretations of primitive and container types.
pub type FieldType = crate::TypeRef;

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
