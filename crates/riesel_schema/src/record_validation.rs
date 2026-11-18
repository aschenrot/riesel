use crate::{FieldType, ModelDefinition, RecordError};
use serde_json::Value;
use uuid::Uuid;

impl ModelDefinition {
    pub fn validate_record(&self, record: &Value) -> Result<(), RecordError> {
        let object = record.as_object().ok_or(RecordError::ExpectedObject)?;

        for (field, kind) in &self.fields {
            if !object.contains_key(field) && !matches!(kind, FieldType::Optional { .. }) {
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

            if !valid_field_value(field_type, value) {
                let expected = field_type_name(field_type);

                return Err(RecordError::InvalidValue {
                    field: field.clone(),
                    expected,
                });
            }
        }

        Ok(())
    }
}

fn valid_field_value(kind: &FieldType, value: &Value) -> bool {
    match kind {
        FieldType::String => value.is_string(),
        FieldType::Bool => value.is_boolean(),
        FieldType::Uuid => value.as_str().is_some_and(|s| Uuid::parse_str(s).is_ok()),
        FieldType::U64 => value.as_u64().is_some(),
        FieldType::I64 => value.as_i64().is_some(),
        FieldType::F64 => value.as_f64().is_some(),
        FieldType::Json => true,
        FieldType::Optional { item } => value.is_null() || valid_field_value(item, value),
        FieldType::List { item } => value
            .as_array()
            .is_some_and(|items| items.iter().all(|value| valid_field_value(item, value))),
        FieldType::Unit | FieldType::Named { .. } => false,
    }
}

fn field_type_name(kind: &FieldType) -> &'static str {
    match kind {
        FieldType::String => "string",
        FieldType::Bool => "bool",
        FieldType::Uuid => "uuid",
        FieldType::U64 => "u64",
        FieldType::I64 => "i64",
        FieldType::F64 => "f64",
        FieldType::Json => "json",
        FieldType::Optional { .. } => "optional",
        FieldType::List { .. } => "list",
        FieldType::Unit | FieldType::Named { .. } => "unsupported model field",
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

    #[test]
    fn nested_numeric_optional_and_list_fields_are_validated() {
        let model = ModelDefinition {
            fields: BTreeMap::from([
                ("count".into(), FieldType::U64),
                ("delta".into(), FieldType::I64),
                ("ratio".into(), FieldType::F64),
                (
                    "notes".into(),
                    FieldType::Optional { item: Box::new(FieldType::String) },
                ),
                ("tags".into(), FieldType::List { item: Box::new(FieldType::String) }),
            ]),
        };
        assert!(
            model
                .validate_record(&json!({"count":1,"delta":-2,"ratio":2.5,"tags":["a","b"]}))
                .is_ok()
        );
        assert!(
            model
                .validate_record(&json!({"count":1,"delta":-2,"ratio":2.5,"notes":null,"tags":[]}))
                .is_ok()
        );
        assert!(matches!(
            model.validate_record(
                &json!({"count":-1,"delta":-2,"ratio":2.5,"notes":"x","tags":[]})
            ),
            Err(RecordError::InvalidValue { field, .. }) if field == "count"
        ));
        assert!(matches!(
            model.validate_record(
                &json!({"count":1,"delta":-2,"ratio":2.5,"tags":["okay",1]})
            ),
            Err(RecordError::InvalidValue { field, .. }) if field == "tags"
        ));
        let json = serde_json::to_value(FieldType::Optional { item: Box::new(FieldType::U64) }).unwrap();
        assert_eq!(json, json!({"kind":"optional","item":{"kind":"u64"}}));
    }
}
