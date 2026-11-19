use riesel_schema::ModelDefinition;
use riesel_storage::StoreError;
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::{collections::BTreeMap, path::Path, sync::Mutex};

const FORMAT_VERSION: i64 = 1;

const TABLES: &str = "
    CREATE TABLE models (
        name TEXT PRIMARY KEY NOT NULL,
        definition BLOB NOT NULL
    ) WITHOUT ROWID;
    CREATE TABLE records (
        model TEXT NOT NULL,
        id BLOB NOT NULL CHECK(length(id)=16),
        revision BLOB NOT NULL CHECK(length(revision)=8),
        data BLOB NOT NULL,
        PRIMARY KEY(model,id)
    ) WITHOUT ROWID;
";

/// SQLite connection owned by exactly one linked provider instance.
pub struct SqliteStore {
    pub(super) connection: Mutex<Connection>,
    pub(super) models: BTreeMap<String, ModelDefinition>,
}

pub(super) fn failed(error: impl std::fmt::Display) -> StoreError {
    StoreError::Failed(format!("SQLite storage failed: {error}"))
}

impl SqliteStore {
    /// Open or initialize explicit local storage; call only during provider startup.
    pub fn open(
        path: impl AsRef<Path>,
        models: BTreeMap<String, ModelDefinition>,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(StoreError::Failed(
                "durable SQLite Storage requires an explicit file path".into(),
            ));
        }
        let mut conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(failed)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(failed)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(failed)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(failed)?;
        let journal: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(failed)?;
        let synchronous: i64 = conn
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .map_err(failed)?;
        if !journal.eq_ignore_ascii_case("wal") || synchronous != 2 {
            return Err(StoreError::Failed(
                "SQLite WAL/FULL durability configuration was not activated".into(),
            ));
        }

        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failed)?;
        let version: i64 = tx
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(failed)?;

        match version {
            0 => {
                let existing: i64 = tx
                    .query_row(
                        "SELECT count(*) FROM sqlite_schema
                         WHERE type='table' AND name NOT LIKE 'sqlite_%'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(failed)?;
                if existing != 0 {
                    return Err(StoreError::Failed(
                        "database already contains an unsupported format".into(),
                    ));
                }
                tx.execute_batch(TABLES).map_err(failed)?;
                for (name, definition) in &models {
                    let bytes = serde_json::to_vec(definition).map_err(failed)?;
                    tx.execute(
                        "INSERT INTO models(name,definition) VALUES(?1,?2)",
                        params![name, bytes],
                    )
                    .map_err(failed)?;
                }
                tx.pragma_update(None, "user_version", FORMAT_VERSION)
                    .map_err(failed)?;
            }
            FORMAT_VERSION => {
                let mut statement = tx
                    .prepare("SELECT name,definition FROM models ORDER BY name")
                    .map_err(failed)?;
                let mut rows = statement.query([]).map_err(failed)?;
                let mut persisted = BTreeMap::new();
                while let Some(row) = rows.next().map_err(failed)? {
                    let name: String = row.get(0).map_err(failed)?;
                    let bytes: Vec<u8> = row.get(1).map_err(failed)?;
                    if persisted.insert(name.clone(), bytes).is_some() {
                        return Err(StoreError::Failed(format!(
                            "duplicate stored model definition: {name}"
                        )));
                    }
                }
                for (name, definition) in &models {
                    let wanted = serde_json::to_vec(definition).map_err(failed)?;
                    match persisted.remove(name) {
                        Some(existing) if existing == wanted => {}
                        _ => {
                            return Err(StoreError::Failed(format!(
                                "stored model definition differs: {name}; migration required"
                            )));
                        }
                    }
                }
                if let Some((unknown, _)) = persisted.into_iter().next() {
                    return Err(StoreError::Failed(format!(
                        "stored model not registered: {unknown}; migration required"
                    )));
                }
            }
            other => {
                return Err(StoreError::Failed(format!(
                    "unsupported SQLite Storage format version {other}"
                )));
            }
        }

        tx.commit().map_err(failed)?;
        Ok(Self {
            connection: Mutex::new(conn),
            models,
        })
    }
}
