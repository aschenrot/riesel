use crate::owner::OwnerGuard;
use riesel_schema::ModelDefinition;
use riesel_storage::{RecordQueryCursors, StoreError};
use rusqlite::{Connection, OpenFlags, TransactionBehavior, params};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Mutex,
};

pub(crate) const FORMAT_VERSION: i64 = 1;

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
    /// Exact serialized definitions cached at provider startup for write fencing.
    expected_models: BTreeMap<String, Vec<u8>>,
    pub(super) readable_models: BTreeSet<String>,
    pub(super) query_cursors: Mutex<RecordQueryCursors>,
    // Dropped after the SQLite connection because fields drop in declaration order.
    owner: OwnerGuard,
}

pub(super) fn failed(error: impl std::fmt::Display) -> StoreError {
    StoreError::Failed(format!("SQLite storage failed: {error}"))
}

/// Compare the whole persisted model set, including names and exact bytes.
/// Call with the SQLite write transaction held so a competing connection cannot
/// change the registration between this check and a record mutation.
pub(crate) fn verify_models(
    conn: &Connection,
    expected: &BTreeMap<String, Vec<u8>>,
) -> Result<(), StoreError> {
    let mut statement = conn
        .prepare("SELECT name,definition FROM models ORDER BY name")
        .map_err(failed)?;
    let mut rows = statement.query([]).map_err(failed)?;
    for (name, definition) in expected {
        let row = rows.next().map_err(failed)?.ok_or_else(|| {
            StoreError::Failed("stored model registration changed; reopen required".into())
        })?;
        let stored_name: String = row.get(0).map_err(failed)?;
        let stored_definition: Vec<u8> = row.get(1).map_err(failed)?;
        if stored_name != *name || stored_definition != *definition {
            return Err(StoreError::Failed(
                "stored model registration changed; reopen required".into(),
            ));
        }
    }
    if rows.next().map_err(failed)?.is_some() {
        return Err(StoreError::Failed(
            "stored model registration changed; reopen required".into(),
        ));
    }
    Ok(())
}

impl SqliteStore {
    /// Reject writes from providers whose cached schema is no longer on disk.
    /// This does not prevent legacy versions or direct SQLite writers.
    pub(super) fn verify_write_registration(&self, conn: &Connection) -> Result<(), StoreError> {
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(failed)?;
        if version != FORMAT_VERSION {
            return Err(StoreError::Failed(
                "SQLite Storage format changed; reopen required".into(),
            ));
        }
        verify_models(conn, &self.expected_models)
    }

    /// Check the lifetime owner before every provider operation; the SQLite
    /// transaction-local schema check remains a separate write fence.
    pub(super) fn verify_owner(&self) -> Result<(), StoreError> {
        self.owner.verify()
    }

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
        let expected_models: BTreeMap<String, Vec<u8>> = models
            .iter()
            .map(|(name, definition)| {
                serde_json::to_vec(definition)
                    .map(|bytes| (name.clone(), bytes))
                    .map_err(failed)
            })
            .collect::<Result<_, _>>()?;
        let mut owner = OwnerGuard::acquire(path)?;
        let mut conn = Connection::open_with_flags(
            owner.database_path(),
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
                for (name, bytes) in &expected_models {
                    tx.execute(
                        "INSERT INTO models(name,definition) VALUES(?1,?2)",
                        params![name, bytes],
                    )
                    .map_err(failed)?;
                }
                tx.pragma_update(None, "user_version", FORMAT_VERSION)
                    .map_err(failed)?;
            }
            FORMAT_VERSION => verify_models(&tx, &expected_models)?,
            other => {
                return Err(StoreError::Failed(format!(
                    "unsupported SQLite Storage format version {other}"
                )));
            }
        }

        tx.commit().map_err(failed)?;
        owner.opened()?;
        Ok(Self {
            connection: Mutex::new(conn),
            models,
            expected_models,
            readable_models: BTreeSet::new(),
            query_cursors: Mutex::new(RecordQueryCursors::default()),
            owner,
        })
    }

    /// Open the first trusted linked reader against an existing SQLite owner
    /// without initializing a database or installing any index/DDL.
    /// The cooperative lifetime lease is shared with writable Storage owners.
    pub fn open_query_only(
        path: impl AsRef<Path>,
        models: BTreeMap<String, ModelDefinition>,
        readable: BTreeSet<String>,
    ) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if path.as_os_str().is_empty() || path == Path::new(":memory:") {
            return Err(StoreError::Failed(
                "Query-only SQLite requires an existing named database".into(),
            ));
        }
        if readable.is_empty() || !readable.is_subset(&models.keys().cloned().collect()) {
            return Err(StoreError::Failed("invalid readable model set".into()));
        }
        // Acquiring the cooperative owner sidecar is not a data mutation.
        // Read-only SQLite flags reject database creation and record writes.
        let mut owner = OwnerGuard::acquire(path)?;
        let conn = Connection::open_with_flags(
            owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(failed)?;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(failed)?;
        if version != FORMAT_VERSION {
            return Err(StoreError::Failed("unsupported SQLite Query format".into()));
        }
        let expected_models: BTreeMap<String, Vec<u8>> = models
            .iter()
            .map(|(name, definition)| {
                serde_json::to_vec(definition)
                    .map(|bytes| (name.clone(), bytes))
                    .map_err(failed)
            })
            .collect::<Result<_, _>>()?;
        verify_models(&conn, &expected_models)?;
        owner.opened()?;
        Ok(Self {
            connection: Mutex::new(conn),
            models,
            expected_models,
            readable_models: readable,
            query_cursors: Mutex::new(RecordQueryCursors::default()),
            owner,
        })
    }

    /// Add read-designated roles to the already-opened *same* linked writer.
    /// The provider factory, not a user-controlled request, chooses visibility.
    pub fn with_query_grants(mut self, readable: BTreeSet<String>) -> Result<Self, StoreError> {
        if readable.is_empty() || !readable.is_subset(&self.models.keys().cloned().collect()) {
            return Err(StoreError::Failed("invalid readable model set".into()));
        }
        self.readable_models = readable;
        Ok(self)
    }
}
