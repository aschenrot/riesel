use serde::Deserialize;
use std::collections::BTreeMap;
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
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    Uuid,
    String,
    Bool,
}

#[derive(Debug, Deserialize)]
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
}

impl AppSchema {
    pub fn validate(&self) -> Result<(), SchemaError> {
        if self.version != 1 {
            return Err(SchemaError::UnsupportedVersion {
                version: self.version,
            });
        }

        Ok(())
    }
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
    InvalidValue { field: String, expected: &'static String },
}