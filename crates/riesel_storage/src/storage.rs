use riesel_macros::{RieselType, capability};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, RieselType)]
pub struct StoredRecord {
    pub id: Uuid,
    pub data: Value,
    pub revision: u64,
}

#[derive(Debug, Error, Serialize, Deserialize, RieselType)]
pub enum StoreError {
    #[error("record not found")]
    NotFound,

    #[error("record was modified since it was read")]
    RevisionConflict,

    #[error("storage operation failed: {0}")]
    Failed(String),
}

/// The public, versioned record-storage capability contract.
///
/// API metadata is generated from this trait and the RieselType derives above.
#[capability(id = "riesel.storage.records", version = 1)]
pub trait RecordStore {
    /// Create a record in a model with a generated identifier and initial revision.
    #[command]
    fn create(&self, model: &str, data: Value) -> Result<StoredRecord, StoreError>;

    /// Retrieve a record by model and identifier, or None if it does not exist.
    #[query]
    fn get(&self, model: &str, id: Uuid) -> Result<Option<StoredRecord>, StoreError>;

    /// List all records belonging to a model.
    #[query]
    fn list(&self, model: &str) -> Result<Vec<StoredRecord>, StoreError>;

    /// Replace record data if the expected revision matches its current revision.
    #[command]
    fn update(
        &self,
        model: &str,
        id: Uuid,
        expected_revision: u64,
        data: Value,
    ) -> Result<StoredRecord, StoreError>;
}

#[cfg(test)]
mod tests {
    use super::RecordStoreContract;
    use riesel_schema::{OperationKind, TypeDefinition, TypeRef};

    #[test]
    fn generates_full_storage_contract_from_rust_declarations() {
        let contract = RecordStoreContract::definition().unwrap();

        assert_eq!(contract.capability.id, "riesel.storage.records");
        assert_eq!(contract.capability.version, 1);
        assert_eq!(contract.operations.len(), 4);
        assert_eq!(contract.operations["create"].kind, OperationKind::Command);
        assert_eq!(contract.operations["get"].kind, OperationKind::Query);
        assert_eq!(contract.operations["list"].kind, OperationKind::Query);
        assert_eq!(contract.operations["update"].kind, OperationKind::Command);

        let update = &contract.operations["update"];
        assert_eq!(
            update
                .inputs
                .iter()
                .map(|arg| arg.name.as_str())
                .collect::<Vec<_>>(),
            vec!["model", "id", "expected_revision", "data"],
        );
        assert_eq!(update.inputs[2].kind, TypeRef::U64);
        assert_eq!(
            update.output,
            TypeRef::Named {
                name: "StoredRecord".into()
            }
        );
        assert_eq!(
            update.error,
            TypeRef::Named {
                name: "StoreError".into()
            }
        );

        assert_eq!(
            contract.operations["get"].output,
            TypeRef::Optional {
                item: Box::new(TypeRef::Named {
                    name: "StoredRecord".into(),
                }),
            },
        );

        let TypeDefinition::Struct { fields } = &contract.types["StoredRecord"] else {
            panic!("StoredRecord must be a struct");
        };
        assert_eq!(fields["revision"], TypeRef::U64);
        assert_eq!(fields["data"], TypeRef::Json);

        let TypeDefinition::Enum { variants } = &contract.types["StoreError"] else {
            panic!("StoreError must be an enum");
        };
        assert_eq!(variants["Failed"], Some(TypeRef::String));
        assert_eq!(variants["NotFound"], None);

        let serialized = serde_json::to_string(&contract).unwrap();
        let restored: riesel_schema::CapabilityContract =
            serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored, contract);
        restored.validate().unwrap();
    }
}
