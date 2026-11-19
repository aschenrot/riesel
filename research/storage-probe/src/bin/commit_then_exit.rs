use redb::{Database, Durability, TableDefinition};
use rusqlite::{Connection, params};
use std::{env, path::Path, process};

const RECORDS: TableDefinition<&str, u64> = TableDefinition::new("records");

fn commit_sqlite(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.pragma_update(None, "synchronous", "FULL").unwrap();
    conn.execute_batch(
        "CREATE TABLE records(model TEXT NOT NULL, id BLOB NOT NULL, revision BLOB NOT NULL,
         data BLOB NOT NULL, PRIMARY KEY(model, id)) WITHOUT ROWID;",
    )
    .unwrap();
    let id = [0xFAu8; 16];
    let revision = 1u64.to_be_bytes();
    conn.execute(
        "INSERT INTO records(model,id,revision,data) VALUES(?1,?2,?3,?4)",
        params!["task", &id[..], &revision[..], &b"committed"[..]],
    )
    .unwrap();
}

fn commit_redb(path: &Path) {
    let db = Database::create(path).unwrap();
    let mut write = db.begin_write().unwrap();
    write.set_durability(Durability::Immediate);
    {
        let mut table = write.open_table(RECORDS).unwrap();
        table.insert("task/after_ack", 1).unwrap();
    }
    write.commit().unwrap();
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert_eq!(args.len(), 3, "usage: commit_then_exit sqlite|redb PATH");
    let path = Path::new(&args[2]);
    match args[1].as_str() {
        "sqlite" => commit_sqlite(path),
        "redb" => commit_redb(path),
        other => panic!("invalid engine {other}"),
    }
    // Exit without dropping DB handles; 137 is distinct from a panic or ordinary exit.
    process::exit(137);
}
