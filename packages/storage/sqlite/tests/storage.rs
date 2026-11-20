use riesel_app::{App, ConnectorRegistry, DeploymentConfig, LocalProviderFactory, ServiceEndpoint};
use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{MAX_PAGE_SIZE, RecordStore, StoreError};
use riesel_storage_sqlite::{SqliteStore, sqlite_storage_provider};
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier},
    thread,
};

fn models() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), TypeRef::String),
                ("completed".into(), TypeRef::Bool),
            ]),
        },
    )])
}

fn task(title: &str) -> serde_json::Value {
    json!({"title": title, "completed": false})
}

#[test]
fn crud_revisions_and_pages_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let store = SqliteStore::open(&path, models()).unwrap();
    let mut ids = Vec::new();
    for index in 0..6 {
        ids.push(
            store
                .create("task", task(&format!("task {index}")))
                .unwrap()
                .id,
        );
    }
    ids.sort();

    let mut cursor = None;
    let mut seen = Vec::new();
    loop {
        let page = store.list_page("task", cursor, 2).unwrap();
        seen.extend(page.records.iter().map(|r| r.id));
        match page.next_cursor {
            Some(after) => cursor = Some(after),
            None => break,
        }
    }
    assert_eq!(seen, ids);
    assert!(matches!(
        store.list_page("task", None, 0),
        Err(StoreError::InvalidPageSize)
    ));
    assert!(matches!(
        store.list_page("task", None, MAX_PAGE_SIZE + 1),
        Err(StoreError::InvalidPageSize)
    ));
    let before = store.get("task", ids[0]).unwrap().unwrap();
    let after = store
        .update("task", before.id, before.revision, task("updated"))
        .unwrap();
    assert_eq!(after.revision, 2);
    assert!(matches!(
        store.update("task", before.id, before.revision, task("stale")),
        Err(StoreError::RevisionConflict)
    ));
    assert!(matches!(
        store.delete("task", before.id, before.revision),
        Err(StoreError::RevisionConflict)
    ));
    drop(store);

    let store = SqliteStore::open(&path, models()).unwrap();
    assert_eq!(
        store.get("task", ids[0]).unwrap().unwrap().data,
        task("updated")
    );
    assert_eq!(store.get("task", ids[0]).unwrap().unwrap().revision, 2);
    store.delete("task", ids[0], 2).unwrap();
    assert!(store.get("task", ids[0]).unwrap().is_none());
    assert!(matches!(
        store.delete("task", ids[0], 2),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn rejects_invalid_unknown_oversized_and_incompatible_models() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let store = SqliteStore::open(&path, models()).unwrap();
    let saved = store.create("task", task("keep")).unwrap();
    assert!(matches!(
        store.create("unknown", task("x")),
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store.create("task", json!({"title": "missing completed"})),
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store.create("task", task(&"a".repeat(65_536))),
        Err(StoreError::Failed(_))
    ));
    drop(store);

    let incompatible = BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([("title".into(), TypeRef::String)]),
        },
    )]);
    let error = SqliteStore::open(&path, incompatible).err().unwrap();
    assert!(matches!(error, StoreError::Failed(_)));
    let store = SqliteStore::open(&path, models()).unwrap();
    assert_eq!(
        store.get("task", saved.id).unwrap().unwrap().data,
        task("keep")
    );
}

#[test]
fn concurrent_expected_revision_mutation_has_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(SqliteStore::open(dir.path().join("tasks.db"), models()).unwrap());
    let record = store.create("task", task("start")).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let threads = (0..2)
        .map(|id| {
            let store = Arc::clone(&store);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.update("task", record.id, 1, task(&format!("winner {id}")))
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(StoreError::RevisionConflict)))
            .count(),
        1
    );
    assert_eq!(store.get("task", record.id).unwrap().unwrap().revision, 2);
}

#[test]
fn preserves_u64_revision_domain_and_checks_overflow() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let store = SqliteStore::open(&path, models()).unwrap();
    let id = store.create("task", task("max")).unwrap().id;
    drop(store);

    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "UPDATE records SET revision=?1 WHERE model='task' AND id=?2",
        rusqlite::params![&u64::MAX.to_be_bytes()[..], &id.as_bytes()[..]],
    )
    .unwrap();
    drop(conn);
    let store = SqliteStore::open(&path, models()).unwrap();
    assert_eq!(store.get("task", id).unwrap().unwrap().revision, u64::MAX);
    assert!(matches!(
        store.update("task", id, u64::MAX, task("overflow")),
        Err(StoreError::Failed(_))
    ));
    store.delete("task", id, u64::MAX).unwrap();
}

#[test]
fn registration_and_preflight_do_not_open_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let provider = sqlite_storage_provider(path.clone(), models()).unwrap();
    assert!(!path.exists());
    let def = provider.definition().unwrap();
    assert_eq!(def.id, "riesel.storage.sqlite");
    assert!(!path.exists());
    let mut connectors = ConnectorRegistry::new();
    connectors.register_local(provider).unwrap();
    assert!(!path.exists());

    let schema = r#"{
        "version":1,
        "app":{"name":"SqliteStartProbe"},
        "models":{"task":{"fields":{"title":{"kind":"string"},"completed":{"kind":"bool"}}}},
        "requires":[{"name":"storage","capability":{"id":"riesel.storage.records","version":2}}]
    }"#;
    let app = App::from_schema(schema).unwrap();
    let deployment = DeploymentConfig::new(BTreeMap::from([(
        "storage".into(),
        ServiceEndpoint::Local {
            provider: "riesel.storage.sqlite".into(),
        },
    )]));
    app.preflight(&deployment, &connectors).unwrap();
    assert!(!path.exists());
    let mut running = app.start(&deployment, &connectors).unwrap();
    assert!(path.exists());
    running.shutdown().unwrap();
}

#[test]
fn invalid_database_path_fails_without_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("absent").join("tasks.db");
    let error = SqliteStore::open(missing, models()).err().unwrap();
    assert!(matches!(error, StoreError::Failed(_)));
}

#[test]
fn rejects_ephemeral_sqlite_paths() {
    for path in [
        std::path::PathBuf::new(),
        std::path::PathBuf::from(":memory:"),
    ] {
        assert!(matches!(
            SqliteStore::open(path, models()),
            Err(StoreError::Failed(_))
        ));
    }
}

#[test]
fn failing_local_provider_startup_preserves_pure_preflight() {
    let dir = tempfile::tempdir().unwrap();
    let inaccessible = dir.path().join("nonexistent").join("tasks.db");
    let provider = sqlite_storage_provider(inaccessible.clone(), models()).unwrap();
    let mut registry = ConnectorRegistry::new();
    registry.register_local(provider).unwrap();
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"MissingDbPath"},"models":{},
            "requires":[{"name":"storage","capability":{"id":"riesel.storage.records","version":2}}]}"#,
    )
    .unwrap();
    let deployment = DeploymentConfig::new(BTreeMap::from([(
        "storage".into(),
        ServiceEndpoint::Local {
            provider: "riesel.storage.sqlite".into(),
        },
    )]));
    app.preflight(&deployment, &registry).unwrap();
    assert!(!inaccessible.exists());
    let error = app.start(&deployment, &registry).err().unwrap();
    assert!(matches!(error, riesel_app::RuntimeError::Connection { .. }));
    assert!(!inaccessible.exists());
}

#[test]
fn committed_write_survives_live_process_exit() {
    // Re-execute only this test as a child to terminate without running Drop
    // on the SqliteStore/Connection. Not an OS or power-loss simulation.
    if let Some(path) = std::env::var_os("RIESEL_SQLITE_TEST_CHILD_PATH") {
        let store = SqliteStore::open(std::path::PathBuf::from(path), models()).unwrap();
        store.create("task", task("committed-before-exit")).unwrap();
        std::process::exit(137);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("abrupt.db");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "committed_write_survives_live_process_exit",
            "--nocapture",
        ])
        .env("RIESEL_SQLITE_TEST_CHILD_PATH", &path)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(137),
        "child did not commit and terminate: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let store = SqliteStore::open(&path, models()).unwrap();
    let page = store.list_page("task", None, 32).unwrap();
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].data, task("committed-before-exit"));
}

fn two_model_registrations() -> BTreeMap<String, ModelDefinition> {
    let mut definitions = models();
    definitions.insert(
        "note".into(),
        ModelDefinition {
            fields: BTreeMap::from([("text".into(), TypeRef::String)]),
        },
    );
    definitions
}

fn raw_revision_and_data(
    conn: &rusqlite::Connection,
    model: &str,
    id: uuid::Uuid,
) -> (Vec<u8>, Vec<u8>) {
    conn.query_row(
        "SELECT revision,data FROM records WHERE model=?1 AND id=?2",
        rusqlite::params![model, id.as_bytes()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .unwrap()
}

#[test]
fn cached_models_fail_closed_on_all_writes_after_external_schema_change() {
    // Every case starts with a valid, opened provider and mutates only SQLite
    // metadata through an independent connection, not an approved migration.
    for drift in [
        "changed_other_model",
        "removed_model",
        "new_model",
        "corrupt_definition",
        "format_version",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("records.db");
        let store = SqliteStore::open(&path, two_model_registrations()).unwrap();
        let saved = store.create("task", task("preserve")).unwrap();
        let note = store.create("note", json!({"text":"unchanged"})).unwrap();

        let conn = rusqlite::Connection::open(&path).unwrap();
        match drift {
            "changed_other_model" => {
                let changed = ModelDefinition {
                    fields: BTreeMap::from([("text".into(), TypeRef::Bool)]),
                };
                conn.execute(
                    "UPDATE models SET definition=?1 WHERE name='note'",
                    rusqlite::params![serde_json::to_vec(&changed).unwrap()],
                )
                .unwrap();
            }
            "removed_model" => {
                conn.execute("DELETE FROM models WHERE name='note'", [])
                    .unwrap();
            }
            "new_model" => {
                conn.execute(
                    "INSERT INTO models(name,definition) VALUES('other',?1)",
                    rusqlite::params![serde_json::to_vec(&models()["task"]).unwrap()],
                )
                .unwrap();
            }
            "corrupt_definition" => {
                conn.execute(
                    "UPDATE models SET definition=?1 WHERE name='note'",
                    rusqlite::params![b"not serialized model metadata"],
                )
                .unwrap();
            }
            "format_version" => {
                conn.pragma_update(None, "user_version", 2).unwrap();
            }
            _ => unreachable!(),
        }
        // Preserve exact persisted data bytes/revisions as well as record count.
        let record_before = raw_revision_and_data(&conn, "task", saved.id);
        let note_before = raw_revision_and_data(&conn, "note", note.id);
        let count: i64 = conn
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))
            .unwrap();

        for mutation in [
            store.create("task", task("rejected")).map(|_| ()),
            store.create("note", json!({"text":"rejected"})).map(|_| ()),
            store
                .update("task", saved.id, saved.revision, task("rejected"))
                .map(|_| ()),
            store.delete("task", saved.id, saved.revision),
        ] {
            assert!(
                matches!(mutation, Err(StoreError::Failed(_))),
                "stale provider accepted a write after {drift}"
            );
        }
        let record_after = raw_revision_and_data(&conn, "task", saved.id);
        let note_after = raw_revision_and_data(&conn, "note", note.id);
        let actual_count: i64 = conn
            .query_row("SELECT count(*) FROM records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(record_before, record_after, "task mutated after {drift}");
        assert_eq!(note_before, note_after, "note mutated after {drift}");
        assert_eq!(count, actual_count, "unexpected new records after {drift}");
    }
}
