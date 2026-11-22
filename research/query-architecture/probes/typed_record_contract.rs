//! #138 isolated research: test a *candidate semantic contract* over unchanged
//! authoritative Memory/SQLite RecordStore v2 providers. This is not a native
//! query engine, public API or proof of managed-provider authorization.
use riesel::{
    RieselModel,
    storage::{RecordStore, StoreError, StoredRecord},
};
use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage_component::MemoryStore;
use riesel_storage_sqlite::SqliteStore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, RieselModel)]
#[riesel(model = "research_query_metadata")]
#[serde(deny_unknown_fields)]
struct ResearchMetadata {
    title: String,
    completed: bool,
    big: u64,
    delta: i64,
    // Missing and explicit null are separately exercised via raw valid
    // record data; serde field modifiers are not supported by RieselModel.
    note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Filter {
    EqU64 {
        field: String,
        value: u64,
    },
    EqBool {
        field: String,
        value: bool,
    },
    EqString {
        field: String,
        value: String,
    },
    Missing {
        field: String,
    },
    Null {
        field: String,
    },
    /// Negative case: this prototype does not claim arbitrary operators.
    UnsupportedRange {
        field: String,
    },
}

impl Filter {
    fn field(&self) -> &str {
        match self {
            Self::EqU64 { field, .. }
            | Self::EqBool { field, .. }
            | Self::EqString { field, .. }
            | Self::Missing { field }
            | Self::Null { field }
            | Self::UnsupportedRange { field } => field,
        }
    }

    fn validate(&self, model: &ModelDefinition) -> Result<(), ProbeError> {
        let Some(kind) = model.fields.get(self.field()) else {
            return Err(ProbeError::InvalidFilter);
        };
        let valid = match self {
            Self::EqU64 { .. } => *kind == TypeRef::U64,
            Self::EqBool { .. } => *kind == TypeRef::Bool,
            Self::EqString { .. } => *kind == TypeRef::String,
            Self::Missing { .. } | Self::Null { .. } => {
                matches!(kind, TypeRef::Optional { .. })
            }
            Self::UnsupportedRange { .. } => false,
        };
        if valid {
            Ok(())
        } else {
            Err(ProbeError::InvalidFilter)
        }
    }

    fn matches(&self, value: &Value) -> bool {
        let field = self.field();
        match self {
            Self::EqU64 { value: target, .. } => {
                value.get(field).and_then(Value::as_u64) == Some(*target)
            }
            Self::EqBool { value: target, .. } => {
                value.get(field).and_then(Value::as_bool) == Some(*target)
            }
            Self::EqString { value: target, .. } => {
                value.get(field).and_then(Value::as_str) == Some(target.as_str())
            }
            Self::Missing { .. } => value.get(field).is_none(),
            Self::Null { .. } => value.get(field).is_some_and(Value::is_null),
            Self::UnsupportedRange { .. } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProbeError {
    InvalidFilter,
    CursorMismatch,
    InvalidBudget,
    Store,
}
impl From<StoreError> for ProbeError {
    fn from(_: StoreError) -> Self {
        Self::Store
    }
}

/// A descriptive research cursor: NOT tamper-resistant or authenticated.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeCursor {
    model: String,
    filter: Filter,
    session: u64,
    after: Uuid,
}

#[derive(Debug)]
struct ProbePage {
    records: Vec<StoredRecord>,
    scanned: usize,
    exhausted: bool,
    next: Option<ProbeCursor>,
}

#[derive(Debug, Clone, Copy)]
struct QueryLimits {
    result_limit: usize,
    scan_budget: usize,
}

/// This candidate executor scans bounded v2 pages; it does not execute native
/// indexed predicates. A partial empty page must never mean "no matches".
fn page(
    store: &dyn RecordStore,
    model: &str,
    definition: &ModelDefinition,
    filter: &Filter,
    cursor: Option<ProbeCursor>,
    session: u64,
    limits: QueryLimits,
) -> Result<ProbePage, ProbeError> {
    let QueryLimits { result_limit, scan_budget } = limits;
    filter.validate(definition)?;
    if !(1..=32).contains(&result_limit) || !(1..=64).contains(&scan_budget) {
        return Err(ProbeError::InvalidBudget);
    }
    if let Some(ref c) = cursor
        && (c.model != model || c.filter != *filter || c.session != session)
    {
        return Err(ProbeError::CursorMismatch);
    }
    let mut after = cursor.map(|c| c.after);
    let mut matches = Vec::new();
    let mut scanned = 0;
    let mut exhausted = false;
    while scanned < scan_budget && matches.len() < result_limit {
        let take = (scan_budget - scanned).min(32) as u64;
        let source = store.list_page(model, after, take)?;
        let length = source.records.len();
        for (i, item) in source.records.into_iter().enumerate() {
            // Stored data is authoritative and already schema-validated by
            // the registered providers, but the predicate interpreter is
            // separate research-only code.
            let matched = filter.matches(&item.data);
            after = Some(item.id);
            scanned += 1;
            if matched {
                matches.push(item);
            }
            if matches.len() == result_limit {
                // If we consumed the entire last Storage page, source's
                // next_cursor is authoritative for current exhaustion.
                exhausted = (i + 1 == length) && source.next_cursor.is_none();
                break;
            }
        }
        if matches.len() == result_limit {
            break;
        }
        if source.next_cursor.is_none() {
            exhausted = true;
            break;
        }
    }
    let next = if exhausted {
        None
    } else {
        after.map(|after| ProbeCursor {
            model: model.to_string(),
            filter: filter.clone(),
            session,
            after,
        })
    };
    Ok(ProbePage {
        records: matches,
        scanned,
        exhausted,
        next,
    })
}

fn build_models() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        ResearchMetadata::MODEL_NAME.to_string(),
        ResearchMetadata::model_definition(),
    )])
}

fn with_each_real_provider(mut check: impl FnMut(&dyn RecordStore)) {
    check(&MemoryStore::with_models(build_models()));
    let dir = tempfile::tempdir().unwrap();
    let sqlite = SqliteStore::open(dir.path().join("query-research.db"), build_models()).unwrap();
    check(&sqlite);
}

fn data(index: usize) -> Value {
    json!({
        "title": format!("asset-{index}"),
        "completed": true,
        "big": u64::MAX - 1,
        "delta": -1,
    })
}

fn lookup(store: &dyn RecordStore, id: Uuid) -> StoredRecord {
    store
        .get(ResearchMetadata::MODEL_NAME, id)
        .unwrap()
        .unwrap()
}

fn seed(store: &dyn RecordStore) -> Vec<Uuid> {
    let mut ids: Vec<_> = (0..65)
        .map(|i| {
            store
                .create(ResearchMetadata::MODEL_NAME, data(i))
                .unwrap()
                .id
        })
        .collect();
    ids.sort_unstable();
    // Place sparse match at last UUID; force numeric/null edge cases at
    // the start so the same predicates are tested against both providers.
    for (pos, replacement) in [
        (
            0,
            json!({"title":"highest","completed":true,"big":u64::MAX,"delta":i64::MIN,"note":null}),
        ),
        (
            1,
            json!({"title":"present","completed":true,"big":u64::MAX-1,"delta":i64::MAX,"note":"x"}),
        ),
        (
            2,
            json!({"title":"null","completed":true,"big":u64::MAX-1,"delta":-1,"note":null}),
        ),
        (
            64,
            json!({"title":"sparse","completed":false,"big":u64::MAX-1,"delta":-1}),
        ),
    ] {
        let previous = lookup(store, ids[pos]);
        store
            .update(
                ResearchMetadata::MODEL_NAME,
                ids[pos],
                previous.revision,
                replacement,
            )
            .unwrap();
    }
    ids
}

#[test]
fn typed_predicates_preserve_full_width_numbers_and_optional_presence() {
    with_each_real_provider(|store| {
        let ids = seed(store);
        let model = ResearchMetadata::model_definition();
        assert_eq!(lookup(store, ids[0]).data["big"].as_u64(), Some(u64::MAX));
        assert_eq!(
            lookup(store, ids[1]).data["big"].as_u64(),
            Some(u64::MAX - 1)
        );
        assert_eq!(lookup(store, ids[0]).data["delta"].as_i64(), Some(i64::MIN));
        assert_eq!(lookup(store, ids[1]).data["delta"].as_i64(), Some(i64::MAX));

        let query = |filter: Filter| {
            page(
                store,
                ResearchMetadata::MODEL_NAME,
                &model,
                &filter,
                None,
                77,
                QueryLimits { result_limit: 32, scan_budget: 64 },
            )
            .unwrap()
        };
        let highest = query(Filter::EqU64 {
            field: "big".into(),
            value: u64::MAX,
        });
        assert_eq!(highest.records.len(), 1);
        assert_eq!(highest.records[0].id, ids[0]);
        assert!(
            !highest.exhausted,
            "the first bounded scan cannot claim complete results"
        );
        let last_highest = page(
            store,
            ResearchMetadata::MODEL_NAME,
            &model,
            &Filter::EqU64 {
                field: "big".into(),
                value: u64::MAX,
            },
            highest.next,
            77,
            QueryLimits { result_limit: 32, scan_budget: 64 },
        )
        .unwrap();
        assert_eq!(last_highest.records.len(), 0);
        assert!(last_highest.exhausted);

        let nulls = query(Filter::Null {
            field: "note".into(),
        });
        assert_eq!(
            nulls.records.iter().map(|v| v.id).collect::<Vec<_>>(),
            vec![ids[0], ids[2]]
        );
        assert!(!nulls.exhausted);
        let final_nulls = page(
            store,
            ResearchMetadata::MODEL_NAME,
            &model,
            &Filter::Null {
                field: "note".into(),
            },
            nulls.next,
            77,
            QueryLimits { result_limit: 32, scan_budget: 64 },
        )
        .unwrap();
        assert_eq!(final_nulls.records.len(), 0);
        assert!(final_nulls.exhausted);

        let missing = query(Filter::Missing {
            field: "note".into(),
        });
        assert_eq!(missing.records.len(), 32);
        assert!(!missing.exhausted);
        assert!(
            missing
                .records
                .iter()
                .all(|item| item.data.get("note").is_none())
        );

        let title = query(Filter::EqString {
            field: "title".into(),
            value: "highest".into(),
        });
        assert_eq!(title.records.len(), 1);
        assert_eq!(title.records[0].id, ids[0]);

        assert_eq!(
            page(
                store,
                ResearchMetadata::MODEL_NAME,
                &model,
                &Filter::EqU64 {
                    field: "completed".into(),
                    value: 1
                },
                None,
                77,
                QueryLimits { result_limit: 10, scan_budget: 32 }
            )
            .unwrap_err(),
            ProbeError::InvalidFilter,
        );
        assert_eq!(
            page(
                store,
                ResearchMetadata::MODEL_NAME,
                &model,
                &Filter::UnsupportedRange {
                    field: "big".into()
                },
                None,
                77,
                QueryLimits { result_limit: 10, scan_budget: 32 }
            )
            .unwrap_err(),
            ProbeError::InvalidFilter,
        );
        assert_eq!(
            page(
                store,
                ResearchMetadata::MODEL_NAME,
                &model,
                &Filter::Missing {
                    field: "title".into()
                },
                None,
                77,
                QueryLimits { result_limit: 10, scan_budget: 32 }
            )
            .unwrap_err(),
            ProbeError::InvalidFilter,
        );
    });
}

#[test]
fn bounded_sparse_scan_and_cursor_binding_are_explicit_on_both_providers() {
    with_each_real_provider(|store| {
        let ids = seed(store);
        let model = ResearchMetadata::model_definition();
        let filter = Filter::EqBool {
            field: "completed".into(),
            value: false,
        };
        let first = page(
            store,
            ResearchMetadata::MODEL_NAME,
            &model,
            &filter,
            None,
            77,
            QueryLimits { result_limit: 1, scan_budget: 32 },
        )
        .unwrap();
        assert_eq!(first.scanned, 32);
        assert!(first.records.is_empty());
        assert!(!first.exhausted);
        let c1 = first.next.clone().unwrap();

        assert_eq!(
            page(
                store,
                ResearchMetadata::MODEL_NAME,
                &model,
                &filter,
                Some(c1.clone()),
                78,
                QueryLimits { result_limit: 1, scan_budget: 32 }
            )
            .unwrap_err(),
            ProbeError::CursorMismatch
        );
        assert_eq!(
            page(
                store,
                ResearchMetadata::MODEL_NAME,
                &model,
                &Filter::EqBool {
                    field: "completed".into(),
                    value: true
                },
                Some(c1.clone()),
                77,
                QueryLimits { result_limit: 1, scan_budget: 32 }
            )
            .unwrap_err(),
            ProbeError::CursorMismatch
        );
        assert_eq!(
            page(
                store,
                "research_wrong_model",
                &model,
                &filter,
                Some(c1.clone()),
                77,
                QueryLimits { result_limit: 1, scan_budget: 32 }
            )
            .unwrap_err(),
            ProbeError::CursorMismatch
        );

        let second = page(
            store,
            ResearchMetadata::MODEL_NAME,
            &model,
            &filter,
            Some(c1),
            77,
            QueryLimits { result_limit: 1, scan_budget: 32 },
        )
        .unwrap();
        assert_eq!(second.scanned, 32);
        assert!(second.records.is_empty());
        assert!(!second.exhausted);
        let third = page(
            store,
            ResearchMetadata::MODEL_NAME,
            &model,
            &filter,
            second.next,
            77,
            QueryLimits { result_limit: 1, scan_budget: 32 },
        )
        .unwrap();
        assert_eq!(third.scanned, 1);
        assert_eq!(
            third.records.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![ids[64]]
        );
        assert!(third.exhausted);
        assert!(third.next.is_none());
    });
}
