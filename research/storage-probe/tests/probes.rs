use redb::{Database, Durability, ReadableDatabase, ReadableTable, TableDefinition};
use rusqlite::{Connection, TransactionBehavior, params};
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::time::Duration;

const REDB: TableDefinition<&str, u64> = TableDefinition::new("records");

fn rev(n: u64) -> [u8; 8] {
    n.to_be_bytes()
}

fn sqlite(path: &std::path::Path) -> Connection {
    let conn = Connection::open(path).unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "synchronous", "FULL").unwrap();
    conn
}

#[test]
fn sqlite_uuid_order_revision_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sqlite.db");
    let conn = sqlite(&path);
    conn.execute_batch(
        "CREATE TABLE records (
           model TEXT NOT NULL,
           id BLOB NOT NULL CHECK(length(id)=16),
           revision BLOB NOT NULL CHECK(length(revision)=8),
           data BLOB NOT NULL,
           PRIMARY KEY(model,id)
         ) WITHOUT ROWID;",
    )
    .unwrap();

    for last in [2u8, 1, 3] {
        let mut id = [0u8; 16];
        id[15] = last;
        conn.execute(
            "INSERT INTO records(model,id,revision,data) VALUES(?1,?2,?3,?4)",
            params!["task", &id[..], &rev(1)[..], &b"initial"[..]],
        )
        .unwrap();
    }
    let mut cursor = [0u8; 16];
    cursor[15] = 1;
    let second: Vec<u8> = conn
        .query_row(
            "SELECT id FROM records WHERE model=?1 AND id>?2 ORDER BY id LIMIT 1",
            params!["task", &cursor[..]],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(second[15], 2);

    let changed = conn
        .execute(
            "UPDATE records SET revision=?1 WHERE model=?2 AND id=?3 AND revision=?4",
            params![&rev(u64::MAX)[..], "task", &cursor[..], &rev(1)[..]],
        )
        .unwrap();
    assert_eq!(changed, 1);
    assert_eq!(
        conn.execute(
            "UPDATE records SET revision=?1 WHERE model=?2 AND id=?3 AND revision=?4",
            params![&rev(2)[..], "task", &cursor[..], &rev(1)[..]],
        )
        .unwrap(),
        0,
    );

    drop(conn);
    let conn = sqlite(&path);
    let loaded: Vec<u8> = conn
        .query_row(
            "SELECT revision FROM records WHERE model=?1 AND id=?2",
            params!["task", &cursor[..]],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(u64::from_be_bytes(loaded.try_into().unwrap()), u64::MAX);
}

#[test]
fn sqlite_competing_writes_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sqlite.db");
    let conn = sqlite(&path);
    conn.execute_batch("CREATE TABLE records(id INTEGER PRIMARY KEY, revision INTEGER);")
        .unwrap();
    conn.execute("INSERT INTO records VALUES(1,1)", []).unwrap();
    drop(conn);

    let barrier = Arc::new(Barrier::new(3));
    let threads = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut conn = sqlite(&path);
                barrier.wait();
                let txn = conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .unwrap();
                let changed = txn
                    .execute(
                        "UPDATE records SET revision=2 WHERE id=1 AND revision=1",
                        [],
                    )
                    .unwrap();
                txn.commit().unwrap();
                changed
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let total: usize = threads.into_iter().map(|thread| thread.join().unwrap()).sum();
    assert_eq!(total, 1);
}

#[test]
fn redb_order_revisions_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("redb.db");
    let db = Database::create(&path).unwrap();
    let mut txn = db.begin_write().unwrap();
    txn.set_durability(Durability::Immediate).unwrap();
    {
        let mut table = txn.open_table(REDB).unwrap();
        for key in ["task/2", "task/1", "task/3"] {
            table.insert(key, 1).unwrap();
        }
    }
    txn.commit().unwrap();

    let txn = db.begin_read().unwrap();
    let table = txn.open_table(REDB).unwrap();
    let keys = table
        .iter()
        .unwrap()
        .map(|entry| entry.unwrap().0.value().to_string())
        .collect::<Vec<_>>();
    assert_eq!(keys, ["task/1", "task/2", "task/3"]);
    drop(table);
    drop(txn);

    let mut txn = db.begin_write().unwrap();
    txn.set_durability(Durability::Immediate).unwrap();
    {
        let mut table = txn.open_table(REDB).unwrap();
        assert_eq!(table.get("task/1").unwrap().unwrap().value(), 1);
        table.insert("task/1", u64::MAX).unwrap();
    }
    txn.commit().unwrap();
    drop(db);

    let db = Database::open(&path).unwrap();
    let txn = db.begin_read().unwrap();
    let table = txn.open_table(REDB).unwrap();
    assert_eq!(table.get("task/1").unwrap().unwrap().value(), u64::MAX);
}

#[test]
fn redb_competing_writes_one_winner() {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(Database::create(dir.path().join("redb.db")).unwrap());
    let txn = db.begin_write().unwrap();
    {
        let mut table = txn.open_table(REDB).unwrap();
        table.insert("task/record", 1).unwrap();
    }
    txn.commit().unwrap();

    let barrier = Arc::new(Barrier::new(3));
    let threads = (0..2)
        .map(|_| {
            let db = db.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let mut txn = db.begin_write().unwrap();
                txn.set_durability(Durability::Immediate).unwrap();
                let result = {
                    let mut table = txn.open_table(REDB).unwrap();
                    let current = table.get("task/record").unwrap().unwrap().value();
                    if current == 1 {
                        table.insert("task/record", 2).unwrap();
                        1
                    } else {
                        0
                    }
                };
                txn.commit().unwrap();
                result
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let total: usize = threads.into_iter().map(|thread| thread.join().unwrap()).sum();
    assert_eq!(total, 1);
}

#[test]
fn committed_write_survives_unclosed_child_process_sqlite_and_redb() {
    for engine in ["sqlite", "redb"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("{engine}.db"));
        let child = Command::new(env!("CARGO_BIN_EXE_commit_then_exit"))
            .arg(engine)
            .arg(&path)
            .status()
            .unwrap();
        assert_eq!(child.code(), Some(137), "{engine} child did not commit and exit as intended");

        match engine {
            "sqlite" => {
                let conn = sqlite(&path);
                let value: Vec<u8> = conn
                    .query_row(
                        "SELECT data FROM records WHERE model='task' AND id=?1",
                        params![&[0xFAu8; 16][..]],
                        |row| row.get(0),
                    )
                    .unwrap();
                assert_eq!(value, b"committed");
            }
            "redb" => {
                let db = Database::open(&path).unwrap();
                let txn = db.begin_read().unwrap();
                let table = txn.open_table(REDB).unwrap();
                assert_eq!(table.get("task/after_ack").unwrap().unwrap().value(), 1);
            }
            _ => unreachable!(),
        }
    }
}
