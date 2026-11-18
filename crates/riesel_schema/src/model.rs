//! Rust-first model metadata used by the application definition builder.
use crate::{FieldType, ModelDefinition, RieselType};

/// Source of model metadata derived from a Rust data structure.
/// Model metadata and serialization must describe the same field names.
pub trait RieselModel {
    const MODEL_NAME: &'static str;

    fn model_definition() -> ModelDefinition;
}

/// A model field is a supported subset of the same structural type system
/// as capability methods. Implement only for serializable record shapes.
pub trait ModelField: RieselType {
    fn field_type() -> FieldType {
        Self::type_ref()
    }
}

impl ModelField for bool {}
impl ModelField for String {}
impl ModelField for uuid::Uuid {}
impl ModelField for u64 {}
impl ModelField for i64 {}
impl ModelField for f64 {}
impl ModelField for serde_json::Value {}
impl<T: ModelField> ModelField for Option<T> {}
impl<T: ModelField> ModelField for Vec<T> {}
