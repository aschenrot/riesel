//! Research-only #107: reproduce the actual Storage v2 list-page limitations
//! through Todo on both memory and linked SQLite providers. No product API.
use riesel::storage::RecordStore;
use riesel::{Records, RieselModel};
use riesel_storage_component::MemoryStore;
use riesel_storage_sqlite::SqliteStore;
use serde_json::to_value;
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Instant};
use todo::{TaskData, Todo};
use uuid::Uuid;

type Store = Arc<dyn RecordStore + Send + Sync>;

#[derive(Clone, Copy, Debug)]
enum Provider {
    Memory,
    Sqlite,
}

fn setup(provider: Provider, database: &Path) -> (Todo, Store) {
    let models = BTreeMap::from([(
        TaskData::MODEL_NAME.to_owned(),
        TaskData::model_definition(),
    )]);
    let store: Store = match provider {
        Provider::Memory => Arc::new(MemoryStore::with_models(models)),
        Provider::Sqlite => Arc::new(SqliteStore::open(database, models).unwrap()),
    };
    (Todo::new(Records::from_store(store.clone())), store)
}

/// Seed valid, schema-checked *persisted* records efficiently. This setup is
/// intentionally not a Todo create command: all records begin completed so a
/// sparse Open list can be positioned precisely in ascending random UUID order.
/// Reopening occurs through the authoritative Todo domain with revision checks.
fn seed(
    todo: &Todo,
    store: &Store,
    provider: Provider,
    count: usize,
    open_positions: &[usize],
) -> Vec<Uuid> {
    assert!(open_positions.iter().all(|position| *position < count));
    let started = Instant::now();
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let value = TaskData {
            title: format!("Research task {index}"),
            completed: true,
        };
        let created = store
            .create(TaskData::MODEL_NAME, to_value(value).unwrap())
            .unwrap();
        ids.push(created.id);
    }
    ids.sort_unstable();
    for &position in open_positions {
        let task = todo.get_task(ids[position]).unwrap().unwrap();
        let updated = todo.set_completed(task.id, task.revision, false).unwrap();
        assert_eq!(updated.id, ids[position]);
        assert!(!updated.value.completed);
    }
    println!(
        "status_research provider={provider:?} setup_records={count} elapsed_ms={}",
        started.elapsed().as_millis()
    );
    ids
}

#[derive(Debug)]
struct Scan {
    matches: Vec<Uuid>,
    next_cursor: Option<Uuid>,
    scanned: usize,
    pages: usize,
    exhausted: bool,
}

/// Candidate A: explicitly bounded scan of existing 32-record UUID pages.
/// Critically, no matching task in this response does NOT mean no matching
/// task exists unless the scan also reports exhausted=true.
fn bounded_scan(todo: &Todo, after: Option<Uuid>, completed: bool, max_pages: usize) -> Scan {
    assert!(max_pages > 0);
    let mut matches = Vec::new();
    let mut cursor = after;
    let mut scanned = 0;
    let mut pages = 0;
    let mut exhausted = false;
    for _ in 0..max_pages {
        let page = todo.list_tasks_page(cursor, 32).unwrap();
        assert!(page.records.len() <= 32);
        assert!(!page.records.is_empty() || page.next_cursor.is_none());
        scanned += page.records.len();
        pages += 1;
        matches.extend(
            page.records
                .iter()
                .filter(|task| task.value.completed == completed)
                .map(|task| task.id),
        );
        match page.next_cursor {
            Some(next) => {
                assert!(cursor.is_none_or(|prior| prior < next));
                cursor = Some(next);
            }
            None => {
                exhausted = true;
                cursor = None;
                break;
            }
        }
    }
    Scan {
        matches,
        next_cursor: cursor,
        scanned,
        pages,
        exhausted,
    }
}

fn drain(todo: &Todo, provider: Provider, completed: bool, max_total_pages: usize) -> Scan {
    let mut after = None;
    let mut aggregate = Scan {
        matches: Vec::new(),
        next_cursor: None,
        scanned: 0,
        pages: 0,
        exhausted: false,
    };
    let started = Instant::now();
    while !aggregate.exhausted {
        assert!(
            aggregate.pages < max_total_pages,
            "scan exceeded page bound"
        );
        let part = bounded_scan(todo, after, completed, 1);
        aggregate.matches.extend(part.matches);
        aggregate.scanned += part.scanned;
        aggregate.pages += part.pages;
        aggregate.exhausted = part.exhausted;
        after = part.next_cursor;
    }
    println!(
        "status_research provider={provider:?} scanned={} matches={} pages={} elapsed_ms={}",
        aggregate.scanned,
        aggregate.matches.len(),
        aggregate.pages,
        started.elapsed().as_millis()
    );
    aggregate
}

#[test]
fn first_32_done_can_hide_the_only_open_task_on_memory_and_sqlite() {
    for provider in [Provider::Memory, Provider::Sqlite] {
        let dir = tempfile::tempdir().unwrap();
        let (todo, store) = setup(provider, &dir.path().join("tasks.db"));
        let ids = seed(&todo, &store, provider, 33, &[32]);

        let first = bounded_scan(&todo, None, false, 1);
        assert_eq!(first.scanned, 32, "{provider:?}");
        assert!(first.matches.is_empty(), "{provider:?}");
        assert!(!first.exhausted, "{provider:?}");
        assert_eq!(first.pages, 1);

        let second = bounded_scan(&todo, first.next_cursor, false, 1);
        assert_eq!(second.scanned, 1, "{provider:?}");
        assert_eq!(second.matches, vec![ids[32]], "{provider:?}");
        assert!(second.exhausted, "{provider:?}");
    }
}

#[test]
fn hundred_mixed_records_require_explicit_cursor_continuation() {
    for provider in [Provider::Memory, Provider::Sqlite] {
        let dir = tempfile::tempdir().unwrap();
        let (todo, store) = setup(provider, &dir.path().join("tasks.db"));
        let ids = seed(&todo, &store, provider, 100, &[70, 98, 99]);

        let first = bounded_scan(&todo, None, false, 2);
        assert_eq!(first.scanned, 64, "{provider:?}");
        assert!(first.matches.is_empty(), "{provider:?}");
        assert!(!first.exhausted, "{provider:?}");

        let complete = drain(&todo, provider, false, 4);
        assert_eq!(complete.scanned, 100, "{provider:?}");
        assert_eq!(complete.pages, 4, "{provider:?}");
        assert_eq!(
            complete.matches,
            vec![ids[70], ids[98], ids[99]],
            "{provider:?}"
        );
        assert!(complete.exhausted);
    }
}

#[test]
fn sparse_ten_thousand_records_require_313_pages_on_memory_and_sqlite() {
    for provider in [Provider::Memory, Provider::Sqlite] {
        let dir = tempfile::tempdir().unwrap();
        let (todo, store) = setup(provider, &dir.path().join("tasks.db"));
        let ids = seed(&todo, &store, provider, 10_000, &[9_999]);

        // A finite eight-page scan reads at most 256 of 10,000 records,
        // reports a partial result, and cannot conclude the Open list is empty.
        let bounded = bounded_scan(&todo, None, false, 8);
        assert_eq!(bounded.scanned, 256, "{provider:?}");
        assert_eq!(bounded.pages, 8, "{provider:?}");
        assert!(bounded.matches.is_empty(), "{provider:?}");
        assert!(!bounded.exhausted, "{provider:?}");

        let full = drain(&todo, provider, false, 314);
        assert_eq!(full.matches, vec![ids[9_999]], "{provider:?}");
        assert_eq!(full.scanned, 10_000, "{provider:?}");
        assert_eq!(full.pages, 313, "{provider:?}");
        assert!(full.exhausted);
    }
}

#[test]
fn status_change_behind_cursor_is_missing_from_non_snapshot_continuation() {
    for provider in [Provider::Memory, Provider::Sqlite] {
        let dir = tempfile::tempdir().unwrap();
        let (todo, store) = setup(provider, &dir.path().join("tasks.db"));
        seed(&todo, &store, provider, 65, &[]);

        // Client A reads and advances past a completed task.
        let first_page = todo.list_tasks_page(None, 32).unwrap();
        let first = &first_page.records[0];
        assert!(first.value.completed);
        let continuation = first_page.next_cursor.expect("more pages");

        // Client B changes it after A passed its cursor; there is no snapshot.
        let reopened = todo.set_completed(first.id, first.revision, false).unwrap();
        assert_eq!(reopened.revision, first.revision + 1);
        assert!(!reopened.value.completed);
        let tail = drain_from_cursor(&todo, continuation);
        assert!(!tail.contains(&first.id), "{provider:?}");
        assert!(!todo.get_task(first.id).unwrap().unwrap().value.completed);

        // A stale write from the original client cannot overwrite client B.
        assert!(todo.set_completed(first.id, first.revision, true).is_err());
    }
}

fn drain_from_cursor(todo: &Todo, cursor: Uuid) -> Vec<Uuid> {
    let mut result = Vec::new();
    let mut after = Some(cursor);
    for _ in 0..4 {
        let part = bounded_scan(todo, after, false, 1);
        result.extend(part.matches);
        if part.exhausted {
            return result;
        }
        after = part.next_cursor;
    }
    panic!("cursor traversal did not exhaust within four pages");
}
