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

impl ModelField for u64 {
    fn field_type() -> FieldType {
        FieldType::U64
    }
}

impl ModelField for i64 {
    fn field_type() -> FieldType {
        FieldType::I64
    }
}

impl ModelField for f64 {
    fn field_type() -> FieldType {
        FieldType::F64
    }
}

impl ModelField for serde_json::Value {
    fn field_type() -> FieldType {
        FieldType::Json
    }
}

impl<T: ModelField> ModelField for Option<T> {
    fn field_type() -> FieldType {
        FieldType::Optional(Box::new(T::field_type()))
    }
}

impl<T: ModelField> ModelField for Vec<T> {
    fn field_type() -> FieldType {
        FieldType::List(Box::new(T::field_type()))
    }
}
