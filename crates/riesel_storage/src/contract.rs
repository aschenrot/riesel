use riesel_schema::{
    CapabilityContract, CapabilityRef, OperationDefinition, OperationKind, TypeDefinition, TypeRef,
};
use std::collections::BTreeMap;

pub const RECORDS_CAPABILITY_ID: &str = "riesel.storage.records";
pub const RECORDS_CAPABILITY_VERSION: u32 = 1;

/// Describes the existing RecordStore interface for inspection.
///
/// This is a provisional explicit descriptor. Future contract generation must
/// keep metadata and Rust signatures in sync without two authored APIs.
pub fn records_contract() -> CapabilityContract {
    let record = TypeRef::Named {
        name: "StoredRecord".into(),
    };

    let types = BTreeMap::from([
        (
            "StoredRecord".into(),
            TypeDefinition::Struct {
                fields: inputs([
                    ("id", TypeRef::Uuid),
                    ("data", TypeRef::Json),
                    ("revision", TypeRef::U64),
                ]),
            },
        ),
        (
            "StoreError".into(),
            TypeDefinition::Enum {
                variants: BTreeMap::from([
                    ("NotFound".into(), None),
                    ("RevisionConflict".into(), None),
                    ("Failed".into(), Some(TypeRef::String)),
                ]),
            },
        ),
    ]);

    let operations = BTreeMap::from([
        (
            "create".into(),
            operation(
                OperationKind::Command,
                "Create a record and assign a record ID and initial revision.",
                inputs([("model", TypeRef::String), ("data", TypeRef::Json)]),
                record.clone(),
            ),
        ),
        (
            "get".into(),
            operation(
                OperationKind::Query,
                "Retrieve one record by model and record ID.",
                inputs([("model", TypeRef::String), ("id", TypeRef::Uuid)]),
                TypeRef::Optional {
                    item: Box::new(record.clone()),
                },
            ),
        ),
        (
            "list".into(),
            operation(
                OperationKind::Query,
                "List records belonging to a model.",
                inputs([("model", TypeRef::String)]),
                TypeRef::List {
                    item: Box::new(record.clone()),
                },
            ),
        ),
        (
            "update".into(),
            operation(
                OperationKind::Command,
                "Replace record data only when the expected revision matches.",
                inputs([
                    ("model", TypeRef::String),
                    ("id", TypeRef::Uuid),
                    ("expected_revision", TypeRef::U64),
                    ("data", TypeRef::Json),
                ]),
                record,
            ),
        ),
    ]);

    CapabilityContract {
        capability: CapabilityRef {
            id: RECORDS_CAPABILITY_ID.into(),
            version: RECORDS_CAPABILITY_VERSION,
        },
        operations,
        types,
    }
}

fn inputs<const N: usize>(entries: [(&str, TypeRef); N]) -> BTreeMap<String, TypeRef> {
    entries
        .into_iter()
        .map(|(name, kind)| (name.to_owned(), kind))
        .collect()
}

fn operation(
    kind: OperationKind,
    description: &str,
    inputs: BTreeMap<String, TypeRef>,
    output: TypeRef,
) -> OperationDefinition {
    OperationDefinition {
        kind,
        description: description.into(),
        inputs,
        output,
        error: Some(TypeRef::Named {
            name: "StoreError".into(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RecordStore, StoreError, StoredRecord, TcpStore};
    use serde_json::Value;
    use uuid::Uuid;

    #[test]
    fn storage_contract_is_valid_and_inspectable() {
        let contract = records_contract();
        contract.validate().unwrap();

        assert_eq!(contract.capability.id, RECORDS_CAPABILITY_ID);
        assert_eq!(contract.operations.len(), 4);

        assert_eq!(contract.operations["create"].kind, OperationKind::Command);
        assert_eq!(contract.operations["get"].kind, OperationKind::Query);
        assert_eq!(contract.operations["list"].kind, OperationKind::Query);
        assert_eq!(contract.operations["update"].kind, OperationKind::Command);

        assert_eq!(
            contract.operations["get"].output,
            TypeRef::Optional {
                item: Box::new(TypeRef::Named {
                    name: "StoredRecord".into(),
                }),
            }
        );

        let json = serde_json::to_string(&contract).unwrap();
        let decoded: CapabilityContract = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, contract);
    }

    #[test]
    fn existing_rust_methods_have_the_declared_signatures() {
        let _: fn(&TcpStore, &str, Value) -> Result<StoredRecord, StoreError> =
            <TcpStore as RecordStore>::create;
        let _: fn(&TcpStore, &str, Uuid) -> Result<Option<StoredRecord>, StoreError> =
            <TcpStore as RecordStore>::get;
        let _: fn(&TcpStore, &str) -> Result<Vec<StoredRecord>, StoreError> =
            <TcpStore as RecordStore>::list;
        let _: fn(&TcpStore, &str, Uuid, u64, Value) -> Result<StoredRecord, StoreError> =
            <TcpStore as RecordStore>::update;
    }
}
