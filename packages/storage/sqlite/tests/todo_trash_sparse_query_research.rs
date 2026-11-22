//! Research-only #119: real SQLite v1 paging under a balanced compound filter.
//! Neither product query v2 nor the eventual Todo lifecycle is implemented here.
use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{
    RecordCompare, RecordPredicate, RecordQuery, RecordQueryRequest, RecordStore,
};
use riesel_storage_sqlite::SqliteStore;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

fn model() -> BTreeMap<String, ModelDefinition> {
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

fn inspect_single_predicate(
    store: &SqliteStore,
    field: &str,
    expected: bool,
) -> (u64, u64, u64, Vec<uuid::Uuid>) {
    let mut request = RecordQueryRequest {
        model: "task".into(),
        predicate: Some(RecordPredicate {
            field: field.into(),
            operator: RecordCompare::Eq,
            value: Some(json!(expected)),
        }),
        limit: 32,
        scan_budget: 256,
        cursor: None,
    };
    let mut calls = 0;
    let mut inspected = 0;
    let mut returned = 0;
    let mut matching = Vec::new();
    loop {
        let page = store.query_page(request.clone()).unwrap();
        calls += 1;
        inspected += page.inspected;
        returned += page.records.len() as u64;
        for record in page.records {
            if record.data["completed"] == false && record.data["trashed"] == false {
                matching.push(record.id);
            }
        }
        if page.exhausted {
            assert!(page.next_cursor.is_none());
            break;
        }
        request.cursor = Some(page.next_cursor.expect("every partial page must continue"));
    }
    (calls, inspected, returned, matching)
}

#[test]
fn balanced_sparse_lifecycle_requires_many_v1_native_pages_at_33_100_and_10000() {
    for count in [33_u64, 100, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("mixed-status.db");
        let store = SqliteStore::open(&db, model())
            .unwrap()
            .with_query_grants(BTreeSet::from(["task".into()]))
            .unwrap();

        let mut rows = Vec::with_capacity(count as usize);
        for index in 0..count {
            // Balanced workload: both independently selectable fields match
            // about half the records, but the conjunction initially matches 0.
            let trashed_open = index % 2 == 0;
            rows.push(
                store
                    .create(
                        "task",
                        json!({
                            "title": format!("Task {index:05}"),
                            "completed": !trashed_open,
                            "trashed": trashed_open,
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
                json!({"title":"Only active Open at last UUID",
                       "completed":false,"trashed":false}),
            )
            .unwrap();
        assert_eq!(target.id, last.id);
        let started = Instant::now();

        // Q3 may choose whichever single-field prefilter looks more selective.
        // Both must still return approximately half the rows before the
        // domain can enforce "active AND open" in this balanced case.
        let completed = inspect_single_predicate(&store, "completed", false);
        let active = inspect_single_predicate(&store, "trashed", false);
        for (field, (calls, inspected, returned, matching)) in
            [("completed", &completed), ("trashed", &active)]
        {
            assert_eq!(*inspected, count, "{field} scans every candidate once");
            assert_eq!(matching.as_slice(), &[target.id], "{field} found exact target");
            assert!(
                *returned >= count / 2,
                "{field} returns at least half the records as first-field matches"
            );
            assert!(
                *calls >= (count / 2).div_ceil(32),
                "{field} is limited to 32 first-field matched rows per native call"
            );
            if count == 10_000 {
                assert!(
                    *calls >= 157,
                    "{field} cannot reach last UUID in 40 native v1 calls"
                );
            }
            eprintln!(
                "todo #119 research: count={count} field={field} calls={calls} inspected={inspected} returned={returned} found={} elapsed_ms={}",
                matching.len(),
                started.elapsed().as_millis(),
            );
        }
        // No second-field predicate, no provider-owned AND, no indexed Query
        // or browser HTTP claim is being implemented by this research test.
    }
}
