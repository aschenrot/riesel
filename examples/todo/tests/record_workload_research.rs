//! #107 research-only: broaden the record capability workload evidence beyond
//! Todo. These tests intentionally use the *unchanged* public RecordStore v2.
//! They do not propose a game-engine API or authorize new Storage semantics.
use riesel::storage::{RecordStore, StoreError};
use riesel::{RieselModel, storage::StoredRecord};
use riesel_storage_component::MemoryStore;
use riesel_storage_sqlite::SqliteStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, RieselModel)]
#[riesel(model = "research_game_inventory")]
#[serde(deny_unknown_fields)]
struct Inventory {
    owner: String,
    coins: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, RieselModel)]
#[riesel(model = "research_editor_asset")]
#[serde(deny_unknown_fields)]
struct AssetMetadata {
    tag: String,
    payload: String,
}

type Store = Arc<dyn RecordStore + Send + Sync>;

fn providers(f: impl Fn(Store)) {
    let models = BTreeMap::from([
        (
            Inventory::MODEL_NAME.to_owned(),
            Inventory::model_definition(),
        ),
        (
            AssetMetadata::MODEL_NAME.to_owned(),
            AssetMetadata::model_definition(),
        ),
    ]);
    f(Arc::new(MemoryStore::with_models(models.clone())));
    let directory = tempfile::tempdir().unwrap();
    f(Arc::new(
        SqliteStore::open(&directory.path().join("research.db"), models).unwrap(),
    ));
}

fn get(store: &Store, model: &str, id: Uuid) -> StoredRecord {
    store.get(model, id).unwrap().unwrap()
}

fn coins(record: &StoredRecord) -> u64 {
    record.data["coins"].as_u64().unwrap()
}

#[test]
fn record_v2_cannot_guarantee_atomic_two_record_inventory_transfer() {
    providers(|store| {
        let a = store
            .create(Inventory::MODEL_NAME, json!({"owner":"a","coins":100}))
            .unwrap();
        let b = store
            .create(Inventory::MODEL_NAME, json!({"owner":"b","coins":10}))
            .unwrap();

        // Another authorized client performs a revision-bumping no-op write.
        // The transfer initiator still holds the stale revision for b.
        let b_current = store
            .update(
                Inventory::MODEL_NAME,
                b.id,
                b.revision,
                json!({"owner":"b","coins":10}),
            )
            .unwrap();
        assert_eq!(b_current.revision, b.revision + 1);

        // Client tries to transfer 10 coins in two independently committed calls.
        let debited = store
            .update(
                Inventory::MODEL_NAME,
                a.id,
                a.revision,
                json!({"owner":"a","coins":90}),
            )
            .unwrap();
        assert_eq!(debited.revision, a.revision + 1);
        assert!(matches!(
            store.update(
                Inventory::MODEL_NAME,
                b.id,
                b.revision,
                json!({"owner":"b","coins":20}),
            ),
            Err(StoreError::RevisionConflict)
        ));
        // Neither provider rolls back the previously committed debit. This is
        // evidence that independent v2 CAS writes do not implement a transfer.
        assert_eq!(coins(&get(&store, Inventory::MODEL_NAME, a.id)), 90);
        assert_eq!(coins(&get(&store, Inventory::MODEL_NAME, b.id)), 10);
    });
}

fn tag(record: &StoredRecord) -> &str {
    record.data["tag"].as_str().unwrap()
}

#[test]
fn editor_metadata_equality_outside_loaded_pages_requires_full_model_walk() {
    providers(|store| {
        let mut ids = Vec::new();
        for index in 0..65 {
            let record = store
                .create(
                    AssetMetadata::MODEL_NAME,
                    json!({"tag":"other","payload":format!("asset-{index}")}),
                )
                .unwrap();
            ids.push(record.id);
        }
        ids.sort_unstable();
        // Place the only desired tag after the first 64 UUID-ordered records.
        let last = get(&store, AssetMetadata::MODEL_NAME, ids[64]);
        store
            .update(
                AssetMetadata::MODEL_NAME,
                last.id,
                last.revision,
                json!({"tag":"terrain","payload":"last"}),
            )
            .unwrap();

        let first = store
            .list_page(AssetMetadata::MODEL_NAME, None, 32)
            .unwrap();
        assert_eq!(first.records.len(), 32);
        assert!(first.records.iter().all(|item| tag(item) != "terrain"));
        let second = store
            .list_page(AssetMetadata::MODEL_NAME, first.next_cursor, 32)
            .unwrap();
        assert_eq!(second.records.len(), 32);
        assert!(second.records.iter().all(|item| tag(item) != "terrain"));
        let third = store
            .list_page(AssetMetadata::MODEL_NAME, second.next_cursor, 32)
            .unwrap();
        assert_eq!(third.records.len(), 1);
        assert_eq!(tag(&third.records[0]), "terrain");
        assert_eq!(third.next_cursor, None);
    });
}

#[test]
fn record_payload_limits_reject_bulk_asset_bytes() {
    providers(|store| {
        // This is intentionally *not* a binary asset API; the example is a
        // valid typed model containing an oversized text payload.
        let oversized: Value = json!({
            "tag":"texture",
            "payload":"x".repeat(80 * 1024),
        });
        assert!(matches!(
            store.create(AssetMetadata::MODEL_NAME, oversized),
            Err(StoreError::Failed(_))
        ));
        // Small metadata remains valid.
        let metadata = store
            .create(
                AssetMetadata::MODEL_NAME,
                json!({"tag":"texture","payload":"resource://texture-a"}),
            )
            .unwrap();
        assert_eq!(tag(&metadata), "texture");
    });
}

#[test]
fn raw_sqlite_json_extraction_is_not_a_full_u64_equality_or_optional_contract() {
    // SQLite-only scratch probe; not a Riesel query implementation. Riesel
    // validates JSON u64 values, including those above SQLite's signed range.
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let greatest = json!({"coins": u64::MAX}).to_string();
    let previous = json!({"coins": u64::MAX - 1}).to_string();
    assert_ne!(greatest, previous);
    let (a, b): (f64, f64) = conn
        .query_row(
            "SELECT json_extract(?1, '$.coins'), json_extract(?2, '$.coins')",
            rusqlite::params![greatest, previous],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(a, b, "SQLite rounds distinct large u64 JSON numbers to f64");

    // Both missing and explicit JSON null extract as SQL NULL. However the
    // registered model validator permits an omitted optional field as well
    // as one explicitly containing null; a future equality/IS NULL query
    // must choose and implement its precise cross-provider semantics.
    let (missing, explicit): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT json_type(?1, '$.note'), json_type(?2, '$.note')",
            rusqlite::params!["{}", r#"{"note":null}"#],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(missing, None);
    assert_eq!(explicit.as_deref(), Some("null"));
    let (missing_value, null_value): (Option<String>, Option<String>) = conn
        .query_row(
            "SELECT json_extract(?1, '$.note'), json_extract(?2, '$.note')",
            rusqlite::params!["{}", r#"{"note":null}"#],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(missing_value, null_value);
}
