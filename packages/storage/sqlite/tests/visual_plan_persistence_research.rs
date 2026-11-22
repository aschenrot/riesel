//! RESEARCH ONLY (#111): persistent V2 semantic/layout CAS and activation
//! receipts against real SQLite connections and a real validated plan.
//! This is not Riesel's public plan store, publisher, authorization mechanism
//! or a guarantee about fsync/power loss or remote process identities.

use riesel_app::{ExecutionPlanDraft, OperationKey, OperationNode};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use std::{path::{Path, PathBuf}, sync::{Arc, Barrier}, thread, time::Duration};
use tempfile::{TempDir, tempdir};

const PLAN_ID: &str = "research.persistent.plan";

fn connect(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.busy_timeout(Duration::from_secs(5)).unwrap();
    connection.pragma_update(None, "synchronous", "FULL").unwrap();
    connection
}

fn plan_document(revision: i64) -> String {
    let plan = ExecutionPlanDraft::new(PLAN_ID, revision as u64)
        .operation(OperationNode::new(
            "op",
            OperationKey::new("research.operation", 1).unwrap(),
        ))
        .validate()
        .unwrap();
    serde_json::to_string(plan.description()).unwrap()
}

fn setup() -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("authoring.sqlite");
    let connection = connect(&path);
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             CREATE TABLE plan_head(
                 plan_id TEXT PRIMARY KEY,
                 semantic_revision INTEGER NOT NULL,
                 active_revision INTEGER NOT NULL,
                 source_name TEXT NOT NULL,
                 source_epoch INTEGER NOT NULL,
                 layout_revision INTEGER NOT NULL
             );
             CREATE TABLE plan_revisions(
                 plan_id TEXT NOT NULL,
                 semantic_revision INTEGER NOT NULL,
                 document TEXT NOT NULL,
                 PRIMARY KEY(plan_id, semantic_revision)
             );
             CREATE TABLE activation_receipts(
                 request_id TEXT PRIMARY KEY,
                 plan_id TEXT NOT NULL,
                 semantic_revision INTEGER NOT NULL
             );",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO plan_head
             (plan_id, semantic_revision, active_revision, source_name, source_epoch, layout_revision)
             VALUES (?1, 1, 1, 'rust', 1, 1)",
            params![PLAN_ID],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO plan_revisions(plan_id, semantic_revision, document)
             VALUES (?1, 1, ?2)",
            params![PLAN_ID, plan_document(1)],
        )
        .unwrap();
    drop(connection);
    (dir, path)
}

fn publish(
    connection: &mut Connection,
    source: &str,
    expected_revision: i64,
    expected_epoch: i64,
) -> bool {
    let next_revision = expected_revision + 1;
    let document = plan_document(next_revision);
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let changed = tx
        .execute(
            "UPDATE plan_head SET semantic_revision=?1
             WHERE plan_id=?2 AND semantic_revision=?3
             AND source_name=?4 AND source_epoch=?5",
            params![next_revision, PLAN_ID, expected_revision, source, expected_epoch],
        )
        .unwrap();
    if changed == 0 {
        tx.rollback().unwrap();
        return false;
    }
    tx.execute(
        "INSERT INTO plan_revisions(plan_id, semantic_revision, document)
         VALUES (?1, ?2, ?3)",
        params![PLAN_ID, next_revision, document],
    )
    .unwrap();
    tx.commit().unwrap();
    true
}

fn transfer(
    connection: &Connection,
    expected_revision: i64,
    expected_epoch: i64,
    represents_all_semantics: bool,
) -> bool {
    if !represents_all_semantics {
        return false;
    }
    connection
        .execute(
            "UPDATE plan_head SET source_name='document', source_epoch=source_epoch+1
             WHERE plan_id=?1 AND semantic_revision=?2 AND source_name='rust'
             AND source_epoch=?3",
            params![PLAN_ID, expected_revision, expected_epoch],
        )
        .unwrap()
        == 1
}

fn move_layout(
    connection: &Connection,
    expected_layout: i64,
    expected_epoch: i64,
    anchored_revision: i64,
) -> bool {
    connection
        .execute(
            "UPDATE plan_head SET layout_revision=layout_revision+1
             WHERE plan_id=?1 AND layout_revision=?2 AND source_epoch=?3
             AND EXISTS (SELECT 1 FROM plan_revisions
                         WHERE plan_id=?1 AND semantic_revision=?4)",
            params![PLAN_ID, expected_layout, expected_epoch, anchored_revision],
        )
        .unwrap()
        == 1
}

/// This is a test-local authorization stand-in. A string/boolean does not
/// prove a remote principal, issuer or protected application identity.
fn activate(
    connection: &mut Connection,
    permitted: bool,
    expected_active: i64,
    requested_revision: i64,
    request_id: &str,
) -> bool {
    if !permitted {
        return false;
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let existing: Option<i64> = tx
        .query_row(
            "SELECT semantic_revision FROM activation_receipts WHERE request_id=?1",
            params![request_id],
            |row| row.get(0),
        )
        .optional()
        .unwrap();
    if let Some(previous) = existing {
        return previous == requested_revision;
    }
    let changed = tx
        .execute(
            "UPDATE plan_head SET active_revision=?1
             WHERE plan_id=?2 AND active_revision=?3
             AND EXISTS (SELECT 1 FROM plan_revisions
                         WHERE plan_id=?2 AND semantic_revision=?1)",
            params![requested_revision, PLAN_ID, expected_active],
        )
        .unwrap();
    if changed != 1 {
        return false;
    }
    tx.execute(
        "INSERT INTO activation_receipts(request_id, plan_id, semantic_revision)
         VALUES (?1, ?2, ?3)",
        params![request_id, PLAN_ID, requested_revision],
    )
    .unwrap();
    tx.commit().unwrap();
    true
}

fn state(connection: &Connection) -> (i64, i64, i64, i64, String) {
    connection
        .query_row(
            "SELECT semantic_revision, active_revision, layout_revision, source_epoch, source_name
             FROM plan_head WHERE plan_id=?1",
            params![PLAN_ID],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap()
}

#[test]
fn independent_connections_race_on_the_same_persistent_semantic_revision() {
    let (_dir, path) = setup();
    let gate = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let gate = Arc::clone(&gate);
        let path = path.clone();
        handles.push(thread::spawn(move || {
            let mut connection = connect(&path);
            gate.wait();
            publish(&mut connection, "rust", 1, 1)
        }));
    }
    gate.wait();
    let accepted = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .filter(|accepted| *accepted)
        .count();
    assert_eq!(accepted, 1);
    let connection = connect(&path);
    assert_eq!(state(&connection), (2, 1, 1, 1, "rust".into()));
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM plan_revisions WHERE plan_id=?1",
            params![PLAN_ID],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 2);
}

#[test]
fn separate_layout_cas_and_source_generation_survive_connection_reopen() {
    let (_dir, path) = setup();
    {
        let connection = connect(&path);
        assert!(move_layout(&connection, 1, 1, 1));
        assert!(!move_layout(&connection, 1, 1, 1));
        assert!(!move_layout(&connection, 2, 1, 99));
        assert!(!transfer(&connection, 1, 1, false));
        assert!(transfer(&connection, 1, 1, true));
        assert!(!move_layout(&connection, 2, 1, 1));
    }
    let mut connection = connect(&path);
    assert_eq!(state(&connection), (1, 1, 2, 2, "document".into()));
    assert!(!publish(&mut connection, "rust", 1, 1));
    assert!(publish(&mut connection, "document", 1, 2));
    assert_eq!(state(&connection), (2, 1, 2, 2, "document".into()));
    assert!(move_layout(&connection, 2, 2, 1)); // Historical anchor is allowed.
    assert_eq!(state(&connection), (2, 1, 3, 2, "document".into()));
}

#[test]
fn uncommitted_activation_rolls_back_on_connection_loss() {
    let (_dir, path) = setup();
    let mut connection = connect(&path);
    assert!(publish(&mut connection, "rust", 1, 1));
    {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        tx.execute(
            "UPDATE plan_head SET active_revision=2 WHERE plan_id=?1",
            params![PLAN_ID],
        )
        .unwrap();
        // Simulate a crashed authoring transaction by dropping without commit.
    }
    drop(connection);
    let connection = connect(&path);
    assert_eq!(state(&connection), (2, 1, 1, 1, "rust".into()));
}

#[test]
fn published_candidate_and_activation_receipt_survive_reopen_after_lost_reply() {
    let (_dir, path) = setup();
    {
        let mut connection = connect(&path);
        assert!(publish(&mut connection, "rust", 1, 1));
        assert_eq!(state(&connection).1, 1); // Publishing did not activate.
        assert!(!activate(&mut connection, false, 1, 2, "request-123"));
        assert!(activate(&mut connection, true, 1, 2, "request-123"));
        // A caller may lose the success reply here. No automatic replay.
    }
    let mut connection = connect(&path);
    assert_eq!(state(&connection), (2, 2, 1, 1, "rust".into()));
    let receipt: i64 = connection
        .query_row(
            "SELECT semantic_revision FROM activation_receipts WHERE request_id=?1",
            params!["request-123"],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(receipt, 2);
    assert!(activate(&mut connection, true, 1, 2, "request-123"));
    assert!(!activate(&mut connection, true, 1, 1, "request-123"));
    assert!(!activate(&mut connection, true, 1, 1, "different-request"));
}

#[test]
fn persistent_candidate_uses_the_actual_read_only_validated_plan_projection() {
    let (_dir, path) = setup();
    let connection = connect(&path);
    let document: String = connection
        .query_row(
            "SELECT document FROM plan_revisions
             WHERE plan_id=?1 AND semantic_revision=1",
            params![PLAN_ID],
            |row| row.get(0),
        )
        .unwrap();
    let projected: serde_json::Value = serde_json::from_str(&document).unwrap();
    assert_eq!(projected["id"], PLAN_ID);
    assert_eq!(projected["revision"], 1);
    assert_eq!(projected["operations"][0]["id"], "op");
    assert!(projected["operations"][0].get("data_edges").is_none());
}
