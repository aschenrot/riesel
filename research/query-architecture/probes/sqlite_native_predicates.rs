//! #138 research-only: SQLite native expression evidence over a database
//! populated by the real, unchanged SqliteStore provider.
//! The provider must be dropped before direct SQLite access. No published
//! RecordStore Query API, provider mutation or operational index migration.
use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::RecordStore;
use riesel_storage_sqlite::SqliteStore;
use rusqlite::{Connection, OpenFlags, Params, params};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};
use uuid::Uuid;

const MODEL: &str = "research_query_metadata";

fn models() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        MODEL.to_string(),
        ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), TypeRef::String),
                ("completed".into(), TypeRef::Bool),
                ("big".into(), TypeRef::U64),
                ("delta".into(), TypeRef::I64),
                (
                    "note".into(),
                    TypeRef::Optional {
                        item: Box::new(TypeRef::String),
                    },
                ),
            ]),
        },
    )])
}

fn fixture() -> (tempfile::TempDir, PathBuf, Vec<Uuid>) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("real-provider-seeded-scratch.sqlite");
    let writer = SqliteStore::open(&path, models()).unwrap();
    let mut ids: Vec<_> = (0..65)
        .map(|i| {
            writer
                .create(
                    MODEL,
                    json!({
                        "title":format!("asset-{i}"),
                        "completed":true,
                        "big":1_u64,
                        "delta":0_i64
                    }),
                )
                .unwrap()
                .id
        })
        .collect();
    ids.sort_unstable();
    for (position, replacement) in [
        (
            0,
            json!({
                "title":"maximum","completed":true,"big":u64::MAX,
                "delta":i64::MIN,"note":null
            }),
        ),
        (
            1,
            json!({
                "title":"previous","completed":true,"big":u64::MAX-1,
                "delta":i64::MAX,"note":"present"
            }),
        ),
        (
            2,
            json!({
                "title":"explicit-null","completed":true,"big":1_u64,
                "delta":0_i64,"note":null
            }),
        ),
        (
            64,
            json!({
                "title":"sparse","completed":false,"big":1_u64,
                "delta":0_i64
            }),
        ),
    ] {
        let original = writer.get(MODEL, ids[position]).unwrap().unwrap();
        writer
            .update(MODEL, ids[position], original.revision, replacement)
            .unwrap();
    }
    // Do not open an independent raw query connection while this provider
    // owns the database; raw SQLite connections do not authenticate to Riesel.
    drop(writer);
    (directory, path, ids)
}

fn select_ids(conn: &Connection, sql: &str, values: impl Params) -> Vec<Uuid> {
    let mut stmt = conn.prepare(sql).unwrap();
    stmt.query_map(values, |row| {
        let bytes: Vec<u8> = row.get(0)?;
        Ok(Uuid::from_slice(&bytes).unwrap())
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn provider_seeded_sqlite_json_token_equality_is_exact_where_extraction_is_not() {
    let (_scratch, path, ids) = fixture();
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let exact_u64 = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         (CAST(data AS TEXT) -> '$.big')=?2 ORDER BY id",
        params![MODEL, u64::MAX.to_string()],
    );
    assert_eq!(exact_u64, vec![ids[0]]);
    let previous = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         (CAST(data AS TEXT) -> '$.big')=?2 ORDER BY id",
        params![MODEL, (u64::MAX - 1).to_string()],
    );
    assert_eq!(previous, vec![ids[1]]);

    // Falsifier: SQLite's numeric JSON extraction rounds both valid u64
    // values to the same floating-point number beyond i64::MAX.
    let rounded = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         json_extract(CAST(data AS TEXT),'$.big')=?2 ORDER BY id",
        params![MODEL, u64::MAX as f64],
    );
    assert_eq!(rounded, vec![ids[0], ids[1]]);

    let signed = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         (CAST(data AS TEXT) -> '$.delta')=?2 ORDER BY id",
        params![MODEL, i64::MIN.to_string()],
    );
    assert_eq!(signed, vec![ids[0]]);
    let signed_max = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         (CAST(data AS TEXT) -> '$.delta')=?2 ORDER BY id",
        params![MODEL, i64::MAX.to_string()],
    );
    assert_eq!(signed_max, vec![ids[1]]);

    let nulls = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         json_type(CAST(data AS TEXT),'$.note')='null' ORDER BY id",
        params![MODEL],
    );
    assert_eq!(nulls, vec![ids[0], ids[2]]);
    let missing = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         json_type(CAST(data AS TEXT),'$.note') IS NULL ORDER BY id",
        params![MODEL],
    );
    assert_eq!(missing.len(), 62);
    assert!(!missing.contains(&ids[0]));
    assert!(!missing.contains(&ids[1]));
    assert!(!missing.contains(&ids[2]));

    let open = select_ids(
        &conn,
        "SELECT id FROM records WHERE model=?1 AND
         json_type(CAST(data AS TEXT),'$.completed')='false' ORDER BY id",
        params![MODEL],
    );
    assert_eq!(open, vec![ids[64]]);
}

#[test]
fn isolated_expression_index_can_support_exact_u64_without_proving_auto_planning() {
    let (_scratch, path, ids) = fixture();
    // Physical schema changes are restricted to this disposable test DB,
    // after the real provider has been closed.
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE INDEX research_exact_big_idx ON
         records(model, (CAST(data AS TEXT) -> '$.big'), id);",
    )
    .unwrap();
    let sql = "SELECT id FROM records INDEXED BY research_exact_big_idx
         WHERE model=?1 AND (CAST(data AS TEXT) -> '$.big')=?2
         ORDER BY id LIMIT 2";
    assert_eq!(
        select_ids(&conn, sql, params![MODEL, u64::MAX.to_string()]),
        vec![ids[0]]
    );
    assert_eq!(
        select_ids(&conn, sql, params![MODEL, (u64::MAX - 1).to_string()]),
        vec![ids[1]]
    );
    let mut stmt = conn
        .prepare(
            "EXPLAIN QUERY PLAN SELECT id FROM records
         INDEXED BY research_exact_big_idx
         WHERE model=?1 AND (CAST(data AS TEXT) -> '$.big')=?2
         ORDER BY id LIMIT 2",
        )
        .unwrap();
    let plans: Vec<String> = stmt
        .query_map(params![MODEL, u64::MAX.to_string()], |row| row.get(3))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(
        plans.iter().any(|p| p.contains("research_exact_big_idx")),
        "{plans:?}"
    );
    println!("forced expression-index plan: {plans:?}");
    // INDEXED BY requires the index, so no automatic planner or latency
    // performance guarantee follows from this experiment.
}

#[test]
fn sqlite_progress_callback_interrupts_only_the_experimental_reader() {
    let (_scratch, path, _ids) = fixture();
    let mut conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.progress_handler(1, Some(|| true)).unwrap();
    let interrupted: rusqlite::Result<i64> = conn.query_row(
        "SELECT count(*) FROM records WHERE model=?1",
        params![MODEL],
        |row| row.get(0),
    );
    assert!(matches!(
        interrupted,
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::OperationInterrupted
    ));
    conn.progress_handler(0, None::<fn() -> bool>).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM records WHERE model=?1",
            params![MODEL],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 65);
    // This proves one SQLite connection can cooperatively abort a query,
    // not that an RPC disconnect automatically cancels a provider call.
}
