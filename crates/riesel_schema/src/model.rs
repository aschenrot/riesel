//! Rust-first model metadata used by the application definition builder.
use crate::{FieldType, ModelDefinition};
use uuid::Uuid;

/// Source of model metadata derived from a Rust data structure.
/// Model metadata and serialization must describe the same field names.
pub trait RieselModel {
    const MODEL_NAME: &'static str;

    fn model_definition() -> ModelDefinition;
}

/// Maps supported Rust field types to the portable Riesel model schema.
/// Unsupported field types produce compile errors when deriving RieselModel.
pub trait ModelField {
    fn field_type() -> FieldType;
}

impl ModelField for bool {
    fn field_type() -> FieldType {
        FieldType::Bool
    }
}

impl ModelField for String {
    fn field_type() -> FieldType {
        FieldType::String
    }
}

impl ModelField for Uuid {
    fn field_type() -> FieldType {
        FieldType::Uuid
    }
}
