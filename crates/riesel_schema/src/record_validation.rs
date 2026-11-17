use crate::{FieldType, ModelDefinition, RecordError};
use serde_json::Value;
use uuid::Uuid;

impl ModelDefinition {
    pub fn validate_record(&self, record: &Value) -> Result<(), RecordError> {
        let object = record.as_object().ok_or(RecordError::ExpectedObject)?;

        for field in self.fields.keys() {
            if !object.contains_key(field) {
                return Err(RecordError::MissingField {
                    field: field.clone(),
                });
            }
        }

        for (field, value) in object {
            let field_type = self
                .fields
                .get(field)
                .ok_or_else(|| RecordError::UnknownField {
                    field: field.clone(),
                })?;

            let valid = match field_type {
                FieldType::String => value.is_string(),
                FieldType::Bool => value.is_boolean(),
                FieldType::Uuid => value.as_str().is_some_and(|s| Uuid::parse_str(s).is_ok()),
            };

            if !valid {
                let expected = match field_type {
                    FieldType::String => "string",
                    FieldType::Bool => "bool",
                    FieldType::Uuid => "uuid",
                };

                return Err(RecordError::InvalidValue {
                    field: field.clone(),
                    expected,
                });
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn validates_task_record() {
        let model = ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), FieldType::String),
                ("completed".into(), FieldType::Bool),
            ]),
        };

        assert!(
            model
                .validate_record(&json!({
                    "title": "Build Riesel",
                    "completed": false
                }))
                .is_ok()
        );

        assert!(matches!(
            model.validate_record(&json!({
                "title": "Build Riesel",
                "completed": "no"
            })),
            Err(RecordError::InvalidValue { .. })
        ));
    }
}
