//! Research-only #119: Memory/SQLite parity for the *existing* single-field
//! record Query and revision-fenced in-place lifecycle transitions.
//! Does not add a production Todo field, compound Query API or Trash endpoint.
use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{
    RecordCompare, RecordPredicate, RecordQuery, RecordQueryError, RecordQueryRequest, RecordStore,
    StoreError,
};
use riesel_storage_component::MemoryStore;
use riesel_storage_sqlite::SqliteStore;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

fn models() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), TypeRef::String),
                ("completed".into(), TypeRef::Bool),
                ("trashed".into(), TypeRef::Bool),
            ]),
        },
    )])
}

fn memory() -> MemoryStore {
    MemoryStore::with_query_models(models(), BTreeSet::from(["task".into()]))
}

fn sqlite(path: &std::path::Path) -> SqliteStore {
    SqliteStore::open(path, models())
        .unwrap()
        .with_query_grants(BTreeSet::from(["task".into()]))
        .unwrap()
}

fn request(
    field: &str,
    limit: u64,
    scan_budget: u64,
    cursor: Option<String>,
) -> RecordQueryRequest {
    RecordQueryRequest {
        model: "task".into(),
        predicate: Some(RecordPredicate {
            field: field.into(),
            operator: RecordCompare::Eq,
            value: Some(json!(false)),
        }),
        limit,
        scan_budget,
        cursor,
    }
}

fn verify_two_clients_and_in_place_restore<S: RecordStore + RecordQuery>(store: &S) {
    // The two observations simulate independent clients of the same owner:
    // one does not receive a revision update from the other's command.
    let first = store
        .create(
            "task",
            json!({"title":"Original","completed":false,"trashed":false}),
        )
        .unwrap();
    let second = store.get("task", first.id).unwrap().unwrap();
    assert_eq!(first.revision, second.revision);

    let moved = store
        .update(
            "task",
            first.id,
            first.revision,
            json!({"title":"Original","completed":false,"trashed":true}),
        )
        .unwrap();
    assert_eq!(moved.id, first.id);
    assert_eq!(moved.revision, first.revision + 1);

    // Query v1 cannot hide deleted Open records by completion alone.
    let completed_only = store
        .query_page(request("completed", 32, 256, None))
        .unwrap();
    assert!(completed_only.records.iter().any(|r| r.id == moved.id));
    let active_only = store.query_page(request("trashed", 32, 256, None)).unwrap();
    assert!(!active_only.records.iter().any(|r| r.id == moved.id));

    // Independent stale clients cannot overwrite or physically delete the row.
    assert!(matches!(
        store.update(
            "task",
            second.id,
            second.revision,
            json!({"title":"Unseen overwrite","completed":true,"trashed":false})
        ),
        Err(StoreError::RevisionConflict)
    ));
    assert!(matches!(
        store.delete("task", second.id, second.revision),
        Err(StoreError::RevisionConflict)
    ));
    let observed = store.get("task", first.id).unwrap().unwrap();
    assert_eq!(observed.id, moved.id);
    assert_eq!(observed.revision, moved.revision);
    assert_eq!(observed.data, moved.data);

    let restored = store
        .update(
            "task",
            moved.id,
            moved.revision,
            json!({"title":"Original","completed":false,"trashed":false}),
        )
        .unwrap();
    assert_eq!(restored.id, first.id);
    assert_eq!(restored.revision, moved.revision + 1);
    assert_eq!(restored.data["title"], "Original");
    assert_eq!(restored.data["completed"], false);
    assert_eq!(restored.data["trashed"], false);

    // Even a client which saw the move needs the new restore revision.
    assert!(matches!(
        store.delete("task", first.id, moved.revision),
        Err(StoreError::RevisionConflict)
    ));

    // Storage itself can delete an active row. Production Todo must therefore
    // constrain its domain/HTTP purge path; Storage cannot enforce the policy.
    store.delete("task", first.id, restored.revision).unwrap();
    assert!(store.get("task", first.id).unwrap().is_none());
}

fn issue_cursor<S: RecordQuery>(store: &S) -> String {
    let first = store.query_page(request("completed", 1, 1, None)).unwrap();
    assert_eq!(first.inspected, 1);
    assert!(!first.exhausted);
    first.next_cursor.expect("partial page requires cursor")
}

fn verify_one_use_and_signature<S: RecordStore + RecordQuery>(store: &S) -> String {
    for i in 0..33 {
        store
            .create(
                "task",
                json!({"title":format!("Cursor row {i}"),"completed":false,"trashed":false}),
            )
            .unwrap();
    }
    let token = issue_cursor(store);
    let wrong_field = request("trashed", 1, 1, Some(token.clone()));
    assert!(matches!(
        store.query_page(wrong_field),
        Err(RecordQueryError::InvalidCursor)
    ));

    let resume = request("completed", 1, 1, Some(token.clone()));
    let resumed = store.query_page(resume.clone()).unwrap();
    assert_eq!(resumed.inspected, 1);
    assert!(matches!(
        store.query_page(resume),
        Err(RecordQueryError::InvalidCursor)
    ));

    // A second *unconsumed* cursor is used to test provider lifetime.
    issue_cursor(store)
}

#[test]
fn memory_and_sqlite_trash_updates_are_revision_fenced_and_cursor_bound() {
    let memory = memory();
    verify_two_clients_and_in_place_restore(&memory);
    let old_memory_token = verify_one_use_and_signature(&memory);
    drop(memory);
    let another_memory_owner = memory();
    assert!(matches!(
        another_memory_owner.query_page(request("completed", 1, 1, Some(old_memory_token))),
        Err(RecordQueryError::InvalidCursor)
    ));

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("owner-bound.db");
    let sqlite = sqlite(&db);
    verify_two_clients_and_in_place_restore(&sqlite);
    let old_sqlite_token = verify_one_use_and_signature(&sqlite);
    drop(sqlite);
    let reopened = sqlite(&db);
    // Persisted records do not make provider-local cursor tokens persistent.
    assert!(matches!(
        reopened.query_page(request("completed", 1, 1, Some(old_sqlite_token))),
        Err(RecordQueryError::InvalidCursor)
    ));
}

fn inspect_v1<S: RecordQuery>(store: &S, field: &str) -> (u64, u64, u64, Vec<Uuid>) {
    let mut cursor = None;
    let (mut pages, mut inspected, mut returned) = (0, 0, 0);
    let mut both_match = Vec::new();
    loop {
        let page = store.query_page(request(field, 32, 256, cursor)).unwrap();
        pages += 1;
        inspected += page.inspected;
        returned += page.records.len() as u64;
        for row in page.records {
            if row.data["completed"] == false && row.data["trashed"] == false {
                both_match.push(row.id);
            }
        }
        if page.exhausted {
            assert!(page.next_cursor.is_none());
            break;
        }
        cursor = Some(page.next_cursor.expect("no cursor for partial scan"));
    }
    (pages, inspected, returned, both_match)
}

#[test]
fn memory_real_native_query_matches_sqlite_sparse_lifecycle_bounds() {
    // The same balanced fixture used by #119's real SQLite test. This is not
    // a compound predicate benchmark: Query v1 selects *one* field, then the
    // research harness checks the other field. No product client does this.
    for count in [33_u64, 100, 10_000] {
        let store = memory();
        let mut rows = Vec::with_capacity(count as usize);
        for index in 0..count {
            let trashed_open = index % 2 == 0;
            rows.push(
                store
                    .create(
                        "task",
                        json!({
                            "title":format!("Task {index:05}"),
                            "completed":!trashed_open,
                            "trashed":trashed_open,
                        }),
                    )
                    .unwrap(),
            );
        }
        rows.sort_by_key(|row| row.id);
        let last = rows.last().unwrap();
        let target = store
            .update(
                "task",
                last.id,
                last.revision,
                json!({"title":"Only active Open at final UUID",
                       "completed":false,"trashed":false}),
            )
            .unwrap();
        for field in ["completed", "trashed"] {
            let (calls, inspected, returned, matching) = inspect_v1(&store, field);
            assert_eq!(inspected, count, "{field} must inspect all candidates");
            assert_eq!(matching, vec![target.id], "{field} returned wrong match");
            assert!(returned >= count / 2, "{field} is not a rare selector");
            assert!(calls >= (count / 2).div_ceil(32));
            if count == 10_000 {
                assert!(calls >= 157, "{field} is not a 40-call native AND");
            }
            eprintln!(
                "todo #119 memory native v1: count={count} field={field} \
                 calls={calls} inspected={inspected} returned={returned}"
            );
        }
    }
}
