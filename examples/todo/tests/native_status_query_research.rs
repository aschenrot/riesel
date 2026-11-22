//! Research-only #107: evaluate the now-shipped Record Query against Todo's
//! actual model on both authoritative Memory and SQLite owners.
//!
//! This is not an HTTP endpoint, frontend migration, public API acceptance,
//! background scan loop, universal index or snapshot-consistency guarantee.
use riesel::RieselModel;
use riesel::storage::{
    RecordCompare, RecordPage, RecordPredicate, RecordQuery, RecordQueryPage, RecordQueryRequest,
    RecordStore, StoredRecord,
};
use riesel_storage_component::MemoryStore;
use riesel_storage_sqlite::SqliteStore;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;
use todo::TaskData;

trait ReadWrite: RecordStore + RecordQuery {}
impl<T: RecordStore + RecordQuery> ReadWrite for T {}

fn for_each_provider(mut scenario: impl FnMut(&str, &dyn ReadWrite)) {
    let model = TaskData::MODEL_NAME.to_owned();
    let schema = BTreeMap::from([(model.clone(), TaskData::model_definition())]);
    let grants = BTreeSet::from([model]);
    let memory = MemoryStore::with_query_models(schema.clone(), grants.clone());
    scenario("memory", &memory);

    let directory = tempfile::tempdir().unwrap();
    let sqlite = SqliteStore::open(directory.path().join("status-research.db"), schema)
        .unwrap()
        .with_query_grants(grants)
        .unwrap();
    scenario("sqlite", &sqlite);
}

/// The IDs must be sorted *after* creation: both providers order by UUID,
/// not by insertion time, task title, or update chronology.
fn seed_done(store: &dyn ReadWrite, count: usize) -> Vec<StoredRecord> {
    let mut records = (0..count)
        .map(|n| {
            store
                .create(
                    TaskData::MODEL_NAME,
                    json!({"title":format!("task-{n:05}"),"completed":true}),
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    records.sort_by_key(|record| record.id);
    records
}

fn change_status(store: &dyn ReadWrite, record: &StoredRecord, completed: bool) {
    store
        .update(
            TaskData::MODEL_NAME,
            record.id,
            record.revision,
            json!({"title":record.data["title"],"completed":completed}),
        )
        .unwrap();
}

fn query(
    store: &dyn ReadWrite,
    completed: bool,
    cursor: Option<String>,
    scan_budget: u64,
) -> RecordQueryPage {
    store
        .query_page(RecordQueryRequest {
            model: TaskData::MODEL_NAME.into(),
            predicate: Some(RecordPredicate {
                field: "completed".into(),
                operator: RecordCompare::Eq,
                value: Some(json!(completed)),
            }),
            limit: 32,
            scan_budget,
            cursor,
        })
        .unwrap()
}

fn legacy_page(store: &dyn ReadWrite) -> RecordPage {
    store.list_page(TaskData::MODEL_NAME, None, 32).unwrap()
}

#[test]
fn sparse_open_after_first_32_completed_is_partial_not_empty() {
    for_each_provider(|name, store| {
        let records = seed_done(store, 33);
        change_status(store, &records[32], false);

        let first_old = legacy_page(store);
        assert_eq!(first_old.records.len(), 32, "{name}");
        assert!(
            first_old
                .records
                .iter()
                .all(|r| r.data["completed"].as_bool() == Some(true))
        );
        assert!(first_old.next_cursor.is_some());

        let first = query(store, false, None, 32);
        assert_eq!(first.inspected, 32, "{name}");
        assert!(first.records.is_empty(), "{name}");
        assert!(!first.exhausted, "{name}");
        let cursor = first.next_cursor.expect("empty partial page must continue");

        let second = query(store, false, Some(cursor), 32);
        assert_eq!(second.inspected, 1, "{name}");
        assert!(second.exhausted, "{name}");
        assert_eq!(second.records.len(), 1, "{name}");
        assert_eq!(second.records[0].id, records[32].id, "{name}");

        // Status views are separate Query identities, never a reused cursor
        // from a different predicate or a hidden result count.
        let done = query(store, true, None, 32);
        assert_eq!(done.records.len(), 32, "{name}");
        assert!(!done.exhausted, "{name}");
    });
}

#[test]
fn sparse_open_in_100_records_advances_four_bounded_pages() {
    for_each_provider(|name, store| {
        let records = seed_done(store, 100);
        for position in [70, 98, 99] {
            change_status(store, &records[position], false);
        }
        let expected: Vec<_> = [70, 98, 99].iter().map(|i| records[*i].id).collect();
        let mut next = None;
        let mut found = Vec::new();
        let mut inspected = 0;
        for page_index in 0..4 {
            let page = query(store, false, next, 32);
            inspected += page.inspected;
            found.extend(page.records.iter().map(|r| r.id));
            assert!(page.records.len() <= 32, "{name}");
            assert!(page.inspected <= 32, "{name}");
            assert_eq!(page.exhausted, page_index == 3, "{name}");
            if page_index < 2 {
                assert!(page.records.is_empty(), "{name}");
            }
            next = page.next_cursor;
            if page_index != 3 {
                assert!(next.is_some(), "{name}");
            }
        }
        assert_eq!(inspected, 100, "{name}");
        assert_eq!(found, expected, "{name}");
    });
}

#[test]
fn reclassified_task_behind_cursor_requires_refresh_not_fake_snapshot() {
    for_each_provider(|name, store| {
        let records = seed_done(store, 33);
        change_status(store, &records[32], false);
        let first = query(store, false, None, 32);
        assert!(first.records.is_empty(), "{name}");
        assert!(!first.exhausted, "{name}");

        // Another authorized caller reopens an already inspected record.
        // Continuing a live UUID seek cannot discover a record behind it.
        change_status(store, &records[0], false);
        let continued = query(store, false, first.next_cursor, 32);
        assert_eq!(continued.records.len(), 1, "{name}");
        assert_eq!(continued.records[0].id, records[32].id, "{name}");
        assert!(continued.exhausted, "{name}");

        // An explicit fresh Query observes both without claiming a stable
        // snapshot or reusing a one-time continuation token.
        let refreshed = query(store, false, None, 32);
        assert_eq!(refreshed.records.len(), 1, "{name}");
        assert_eq!(refreshed.records[0].id, records[0].id, "{name}");
        let final_page = query(store, false, refreshed.next_cursor, 32);
        assert_eq!(final_page.records.len(), 1, "{name}");
        assert_eq!(final_page.records[0].id, records[32].id, "{name}");
    });
}

#[test]
fn provider_cursor_is_filter_scoped_and_one_use_across_http_like_reads() {
    use riesel::storage::RecordQueryError;

    for_each_provider(|name, store| {
        seed_done(store, 33);
        let first = query(store, false, None, 32);
        let token = first.next_cursor.expect("partial page cursor");

        let request = |completed: bool, cursor: String| RecordQueryRequest {
            model: TaskData::MODEL_NAME.into(),
            predicate: Some(RecordPredicate {
                field: "completed".into(),
                operator: RecordCompare::Eq,
                value: Some(json!(completed)),
            }),
            limit: 32,
            scan_budget: 32,
            cursor: Some(cursor),
        };

        let wrong_filter = store.query_page(request(true, token.clone()));
        assert!(
            matches!(wrong_filter, Err(RecordQueryError::InvalidCursor)),
            "{name}"
        );

        // Rejected wrong-filter use does not consume a valid cursor. A
        // successful use does consume it, including if the HTTP reply is lost.
        let resumed = store.query_page(request(false, token.clone())).unwrap();
        assert!(resumed.exhausted, "{name}");
        assert!(resumed.records.is_empty(), "{name}");
        let replayed = store.query_page(request(false, token));
        assert!(
            matches!(replayed, Err(RecordQueryError::InvalidCursor)),
            "{name}"
        );
    });
}

fn check_linked_todo_sdk_application(
    reader: riesel::ModelRead<TaskData, riesel::storage::RecordQueryContract>,
    writer: riesel::Records<TaskData>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use todo::Todo;

    let todo = Todo::new(writer);
    let open = todo.create_task("Open from domain")?;
    let done = todo.create_task("Completed from domain")?;
    let completed = todo.set_completed(done.id, done.revision, true)?;

    let predicate = Some(RecordPredicate {
        field: "completed".into(),
        operator: RecordCompare::Eq,
        value: Some(json!(false)),
    });
    let first = reader.page(predicate.clone(), 32, 256, None)?;
    assert_eq!(first.records.len(), 1);
    assert_eq!(first.records[0].id, open.id);
    assert!(first.exhausted);

    // This is the actual generated Query handle and the same real writer,
    // not a test-only one-off observer, synthetic capability or a second DB.
    todo.set_completed(completed.id, completed.revision, false)?;
    let refreshed = reader.page(predicate, 32, 256, None)?;
    let ids: Vec<_> = refreshed.records.iter().map(|task| task.id).collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&open.id));
    assert!(ids.contains(&completed.id));
    assert!(refreshed.exhausted);
    Ok(())
}

#[test]
fn todo_domain_and_generated_sdk_query_share_real_linked_owner() {
    use riesel::Application;

    Application::new("TodoStatusResearchMemory")
        .handler_with_aliases(check_linked_todo_sdk_application, ["read", "write"])
        .unwrap()
        .development()
        .unwrap()
        .run()
        .unwrap();

    let directory = tempfile::tempdir().unwrap();
    Application::new("TodoStatusResearchSqlite")
        .handler_with_aliases(check_linked_todo_sdk_application, ["read", "write"])
        .unwrap()
        .sqlite_storage(directory.path().join("todo-status-sdk.db"))
        .unwrap()
        .run()
        .unwrap();
}

/// Slow, explicit growth-horizon research; run in the dedicated workflow,
/// not in routine workspace tests. Timings omit browser/HTTP cost, are not
/// reproducible SLAs, and must never be used as promised UI performance.
#[test]
#[ignore = "10,000-record growth-horizon research: run explicitly"]
fn ten_thousand_last_open_requires_forty_bounded_query_calls() {
    for_each_provider(|name, store| {
        let setup_start = Instant::now();
        let records = seed_done(store, 10_000);
        change_status(store, &records[9_999], false);
        let setup_time = setup_start.elapsed();
        let started = Instant::now();

        // This is a reproducible worst-case probe, NOT recommended server
        // implementation: no HTTP request should run this unbounded loop.
        let mut cursor = None;
        let mut calls = 0usize;
        let mut total_inspected = 0u64;
        let mut matches = Vec::new();
        loop {
            let page = query(store, false, cursor, 256);
            calls += 1;
            total_inspected += page.inspected;
            assert!(page.inspected <= 256, "{name}");
            matches.extend(page.records.iter().map(|r| r.id));
            if page.exhausted {
                assert!(page.next_cursor.is_none(), "{name}");
                break;
            }
            if calls <= 39 {
                assert!(page.records.is_empty(), "{name}");
            }
            cursor = page.next_cursor;
            assert!(cursor.is_some(), "nonexhausted Query must continue");
            assert!(calls <= 40, "provider failed to advance bounded cursor");
        }
        let scan_time = started.elapsed();
        assert_eq!(calls, 40, "{name}");
        assert_eq!(total_inspected, 10_000, "{name}");
        assert_eq!(matches, vec![records[9_999].id], "{name}");
        println!(
            "research #107 provider={name}: setup={setup_time:?}, scan={scan_time:?}, candidates={total_inspected}, calls={calls}; not HTTP/SLA"
        );
    });
}
