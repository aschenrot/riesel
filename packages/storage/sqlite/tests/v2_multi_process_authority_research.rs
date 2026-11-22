//! #193 research-only: REAL independent OS processes racing one V2 plan head.
//! Uses real riesel_app validated plan descriptions and SQLite transactions.
//! This is NOT a public authority store, credential, app publisher or ABI.
//! No assertion here proves unexpected power-loss, secure remote identity,
//! exactly-once effects or activation of an actual Riesel graph runner.

use riesel_app::{ExecutionPlanDraft, OperationKey, OperationNode};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

const ID: &str = "research.v2";

fn open(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.busy_timeout(Duration::from_secs(8)).unwrap();
    connection.pragma_update(None, "synchronous", "FULL").unwrap();
    connection
}

fn real_plan(revision: u64) -> String {
    let plan = ExecutionPlanDraft::new(ID, revision)
        .operation(OperationNode::new(
            "load",
            OperationKey::new("research.load", 1).unwrap(),
        ))
        .validate()
        .unwrap();
    serde_json::to_string(plan.description()).unwrap()
}

fn setup(path: &Path) {
    let connection = open(path);
    connection.execute_batch(
        "PRAGMA journal_mode=WAL;
        CREATE TABLE head(
            plan_id TEXT PRIMARY KEY,
            semantic_revision INTEGER NOT NULL,
            layout_revision INTEGER NOT NULL,
            source_epoch INTEGER NOT NULL,
            source TEXT NOT NULL,
            active_revision INTEGER NOT NULL
        );
        CREATE TABLE revisions(
            plan_id TEXT NOT NULL,
            revision INTEGER NOT NULL,
            document TEXT NOT NULL,
            PRIMARY KEY(plan_id, revision)
        );
        CREATE TABLE layouts(
            plan_id TEXT NOT NULL,
            layout_revision INTEGER NOT NULL,
            semantic_anchor INTEGER NOT NULL,
            PRIMARY KEY(plan_id,layout_revision)
        );
        CREATE TABLE receipts(
            request_id TEXT PRIMARY KEY,
            revision INTEGER NOT NULL
        );",
    ).unwrap();
    connection.execute(
        "INSERT INTO head VALUES(?1,1,1,1,'rust',1)",
        params![ID],
    ).unwrap();
    connection.execute(
        "INSERT INTO revisions VALUES(?1,1,?2)",
        params![ID, real_plan(1)],
    ).unwrap();
    connection.execute(
        "INSERT INTO layouts VALUES(?1,1,1)",
        params![ID],
    ).unwrap();
}

fn state(path: &Path) -> (i64, i64, i64, String, i64) {
    open(path).query_row(
        "SELECT semantic_revision,layout_revision,source_epoch,source,active_revision FROM head WHERE plan_id=?1",
        params![ID],
        |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
    ).unwrap()
}

fn cas_semantic(path: &Path, expected: i64, epoch: i64, source: &str) -> bool {
    let mut conn = open(path);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    let next = expected + 1;
    if tx.execute(
        "UPDATE head SET semantic_revision=?1 WHERE plan_id=?2
        AND semantic_revision=?3 AND source_epoch=?4 AND source=?5",
        params![next,ID,expected,epoch,source],
    ).unwrap() == 0 {
        return false;
    }
    tx.execute(
        "INSERT INTO revisions VALUES(?1,?2,?3)",
        params![ID,next,real_plan(next as u64)],
    ).unwrap();
    tx.commit().unwrap();
    true
}

fn cas_layout(path: &Path, expected: i64, epoch: i64, anchor: i64) -> bool {
    let mut conn = open(path);
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    if tx.execute(
        "UPDATE head SET layout_revision=layout_revision+1 WHERE plan_id=?1
        AND layout_revision=?2 AND source_epoch=?3
        AND EXISTS (SELECT 1 FROM revisions WHERE plan_id=?1 AND revision=?4)",
        params![ID,expected,epoch,anchor],
    ).unwrap() == 0 {
        return false;
    }
    tx.execute(
        "INSERT INTO layouts VALUES(?1,?2,?3)",
        params![ID,expected+1,anchor],
    ).unwrap();
    tx.commit().unwrap();
    true
}

fn transfer(path: &Path, expected_semantic: i64, expected_epoch: i64, representable: bool) -> bool {
    if !representable {
        return false;
    }
    open(path).execute(
        "UPDATE head SET source='document',source_epoch=source_epoch+1
         WHERE plan_id=?1 AND semantic_revision=?2 AND source_epoch=?3 AND source='rust'",
        params![ID,expected_semantic,expected_epoch],
    ).unwrap() == 1
}

fn activation_commit(path: &Path) {
    let mut connection = open(path);
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    tx.execute(
        "UPDATE head SET active_revision=2 WHERE plan_id=?1 AND active_revision=1",
        params![ID],
    ).unwrap();
    tx.execute("INSERT INTO receipts VALUES('request-1',2)",[]).unwrap();
    tx.commit().unwrap();
}

fn wait_for(path: &Path, timeout: Duration) {
    let start=Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < timeout,"test synchronization exceeded bounded deadline: {path:?}");
        thread::sleep(Duration::from_millis(10));
    }
}

fn child(
    db: &Path,
    sync: &Path,
    name: &str,
    mode: &str,
) -> Child {
    Command::new(env::current_exe().unwrap())
        .arg("--exact")
        .arg("subprocess_worker")
        .arg("--nocapture")
        .env("RIESEL_193_DB", db)
        .env("RIESEL_193_SYNC", sync)
        .env("RIESEL_193_NAME", name)
        .env("RIESEL_193_MODE", mode)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Runs only when selected by parent with explicit private env vars.
#[test]
fn subprocess_worker() {
    let Ok(db) = env::var("RIESEL_193_DB") else {
        return;
    };
    let sync=PathBuf::from(env::var("RIESEL_193_SYNC").unwrap());
    let name=env::var("RIESEL_193_NAME").unwrap();
    let mode=env::var("RIESEL_193_MODE").unwrap();
    let db=PathBuf::from(db);
    let ready=sync.join(format!("ready-{name}"));
    match mode.as_str() {
        "semantic" | "layout" => {
            File::create(&ready).unwrap();
            wait_for(&sync.join("go"),Duration::from_secs(12));
            let won=if mode=="semantic" {
                cas_semantic(&db,1,1,"rust")
            } else {
                cas_layout(&db,1,1,1)
            };
            println!("WRITER_OUTCOME={}", if won {"won"} else {"conflict"});
        }
        "uncommitted" => {
            let mut connection=open(&db);
            let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
            tx.execute("UPDATE head SET semantic_revision=2 WHERE plan_id=?1",params![ID]).unwrap();
            File::create(&ready).unwrap();
            wait_for(&sync.join("go"),Duration::from_secs(12));
            // Never commit: parent kills process while write is in progress.
        }
        "committed" => {
            activation_commit(&db);
            File::create(&ready).unwrap();
            wait_for(&sync.join("go"),Duration::from_secs(12));
        }
        _ => panic!("unsupported research worker mode"),
    }
}

fn race(mode: &str) {
    let dir=tempdir().unwrap();
    let db=dir.path().join("v2.sqlite");
    let sync=dir.path().join("sync");
    fs::create_dir(&sync).unwrap();
    setup(&db);
    let workers=vec![
        child(&db,&sync,"a",mode),
        child(&db,&sync,"b",mode),
    ];
    wait_for(&sync.join("ready-a"),Duration::from_secs(15));
    wait_for(&sync.join("ready-b"),Duration::from_secs(15));
    File::create(sync.join("go")).unwrap();
    let mut won=0;
    for worker in workers {
        let output=worker.wait_with_output().unwrap();
        assert!(output.status.success(),
            "child process failed: {}",String::from_utf8_lossy(&output.stderr));
        let stdout=String::from_utf8_lossy(&output.stdout);
        if stdout.contains("WRITER_OUTCOME=won") {
            won+=1;
        } else {
            assert!(stdout.contains("WRITER_OUTCOME=conflict"),"{stdout}");
        }
    }
    assert_eq!(won,1,"one independent OS process must win the CAS");
    let observed=state(&db);
    if mode=="semantic" {
        assert_eq!(observed,(2,1,1,"rust".into(),1));
        let count: i64=open(&db).query_row(
            "SELECT COUNT(*) FROM revisions",[],|row|row.get(0)
        ).unwrap();
        assert_eq!(count,2);
    } else {
        assert_eq!(observed,(1,2,1,"rust".into(),1));
        let count: i64=open(&db).query_row(
            "SELECT COUNT(*) FROM layouts",[],|row|row.get(0)
        ).unwrap();
        assert_eq!(count,2);
    }
}

#[test]
fn two_real_os_processes_compete_on_semantic_predecessor() {
    race("semantic");
}

#[test]
fn two_real_os_processes_compete_on_independent_layout_predecessor() {
    race("layout");
}

#[test]
fn killed_writer_before_commit_rolls_back_actual_sqlite_transaction() {
    let dir=tempdir().unwrap();
    let db=dir.path().join("v2.sqlite");
    let sync=dir.path().join("sync");
    fs::create_dir(&sync).unwrap();
    setup(&db);
    let mut worker=child(&db,&sync,"uncommitted","uncommitted");
    wait_for(&sync.join("ready-uncommitted"),Duration::from_secs(15));
    worker.kill().unwrap();
    let _=worker.wait().unwrap();
    assert_eq!(state(&db),(1,1,1,"rust".into(),1));
    assert!(cas_semantic(&db,1,1,"rust"));
}

#[test]
fn committed_activation_pointer_and_receipt_survive_worker_kill_and_reopen() {
    let dir=tempdir().unwrap();
    let db=dir.path().join("v2.sqlite");
    let sync=dir.path().join("sync");
    fs::create_dir(&sync).unwrap();
    setup(&db);
    assert!(cas_semantic(&db,1,1,"rust"));
    let mut worker=child(&db,&sync,"committed","committed");
    wait_for(&sync.join("ready-committed"),Duration::from_secs(15));
    worker.kill().unwrap();
    let _=worker.wait().unwrap();
    assert_eq!(state(&db),(2,1,1,"rust".into(),2));
    let revision: Option<i64>=open(&db)
        .query_row(
            "SELECT revision FROM receipts WHERE request_id='request-1'",
            [],
            |row|row.get(0),
        )
        .optional().unwrap();
    assert_eq!(revision,Some(2));
}

#[test]
fn changed_source_epoch_fences_stale_writers_after_new_connection() {
    let dir=tempdir().unwrap();
    let db=dir.path().join("v2.sqlite");
    setup(&db);
    assert!(!transfer(&db,1,1,false));
    assert!(transfer(&db,1,1,true));
    assert!(!cas_semantic(&db,1,1,"rust"));
    assert!(!cas_layout(&db,1,1,1));
    assert!(cas_semantic(&db,1,2,"document"));
    assert!(cas_layout(&db,1,2,1));
    assert_eq!(state(&db),(2,2,2,"document".into(),1));
}
