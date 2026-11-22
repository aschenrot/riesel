# Linked Record Query — first trusted linked implementation profile

**Status:** first linked-profile implementation and its integration evidence are tracked in [PR #177](https://github.com/aschenrot/riesel/pull/177) and [#161](https://github.com/aschenrot/riesel/issues/161). This is not an independent-process publication or general production-readiness claim.  
**Owner-accepted architecture:** [Linked Record Query contract](record-query-architecture.md), [#138](https://github.com/aschenrot/riesel/issues/138).  
**Implementation acceptance:** [#161](https://github.com/aschenrot/riesel/issues/161), dependent on the merged [#160](https://github.com/aschenrot/riesel/issues/160) typed model/owner foundation.

## Read-only and read/write composition

The first linked `riesel.query.records@1` capability exposes **one read operation** (`query_page`), independently versioned from Storage v2. It has **no** create, update or delete methods. A Rust model is registered by the existing `ModelRead<T, RecordQueryContract>` parameter or explicit `record_query_at::<T, RecordQueryContract>()` builder. A registered model does not imply a write grant or exported endpoint.

A Query-only model handler is declared in Rust:

```rust
use riesel::{Application, ModelRead, RieselModel};
use riesel::storage::{RecordPredicate, RecordQueryContract};
use serde::Deserialize;

#[derive(Deserialize, RieselModel)]
#[riesel(model = "task")]
struct Task {
    title: String,
    completed: bool,
}

fn read_tasks() -> Result<(), riesel::SdkError> {
    Application::new("TaskReader")
        .handler(
            |reader: ModelRead<Task, RecordQueryContract>| -> Result<
                (),
                Box<dyn std::error::Error + Send + Sync>
            > {
                let mut cursor = None;
                loop {
                    let page = reader.page(
                        None::<RecordPredicate>, // or a supported typed predicate
                        32,   // maximum returned records per call
                        128,  // maximum candidate records inspected per call
                        cursor,
                    )?;
                    for record in &page.records {
                        println!("{}", record.value.title);
                    }
                    if page.exhausted {
                        break;
                    }
                    cursor = page.next_cursor;
                }
                Ok(())
            },
        )?
        .sqlite_record_query("tasks.db")? // existing file; physically read-only
        .run()
}
```

A handler needing mutations composes `ModelRead<Task, RecordQueryContract>` with `Records<Task>` and uses `sqlite_storage(path)` or `development()`. The SDK declares one linked per-model reader/writer owner relation, and the runtime rejects different providers or unsupported placements **before constructing provider state**. The two facets use the same instantiated owner and session. A successful completed Storage v2 write is visible to a later live Query read in that session.

`ModelRead::page(...)` returns `QueryPage<T>`, with validated Rust `Record<T>` values, `next_cursor: Option<String>`, `exhausted: bool` and `inspected: u64`. `ModelRead::service()` also exposes the underlying generated read-only facet when callers need its raw `RecordQueryPage` and its exact JSON presence/null behavior. The typed `Option<T>` deserializer may intentionally represent missing and null as the same Rust `None`; predicate semantics continue to distinguish them at the provider.

## First supported predicate and paging profile

- One optional field predicate per call. `RecordCompare` supports `Eq` and `Ne` on bool, string and normalized UUID; `Eq`, `Ne`, `Gt`, `Ge`, `Lt`, `Le` on `u64`/`i64`; and `IsMissing`, `IsNull` for optional fields plus `IsPresent` for declared fields. Missing or null never silently satisfies a scalar inequality. Comparing directly with JSON null is invalid; use `IsNull`. Unsupported field types/operators and invalid operands fail explicitly.
- Results have a deterministic ascending UUID key order **within one live scan**. The first profile does **not** promise arbitrary field ordering, multi-field boolean expression trees, SQL, joins, F64/JSON comparison, cross-page snapshot isolation, automatic materialized indexes or stable results under concurrent writes. These require explicit later profiles and tests.
- Request-envelope bounds also apply to in-process callers: model and field identifiers are at most 256 UTF-8 bytes, and scalar string operands at most 64 KiB. Oversize inputs fail as `RequestTooLarge` before provider model lookup, rather than being retained in unbounded cursor state. At most 256 active cursors retain signatures with these finite limits.
- Each request must bound both returned rows (`1..=32`) and inspected candidates (`1..=256`). A sparse predicate may return **zero matches with `exhausted=false`** and a continuation: that is **not** a complete empty result. Do not stop pagination based on `records.is_empty()`; stop only when `exhausted` is true. Exactly filled pages may require a final empty completed page.
- Continuations are opaque, one-use, scoped to the exact model/predicate/limits and actual provider instance. The first implementation limits active cursor state to 256, expires idle cursors after five minutes and fails with `InvalidCursor` or `CursorCapacity` explicitly. Application-session invalidation fences escaped service handles. Cursors cannot be replayed after an application/provider restart and are not an external authorization credential.
- A requested model must be registered **and explicitly permitted** at the provider's Query boundary. `ModelDenied` is not converted to an empty successful result. SQLite Query-only attachment checks only its permitted schema subset, does not enumerate other persisted models, opens the existing database physically read-only and does **not** perform startup DDL/index migration. Query/Storage together retain Storage v2 file and CRUD compatibility.

## Trust, deployment and evolution boundaries

The **first profile is trusted linked, same-application-session only**. The link relation cannot authorize a separate process, browser, GUI or managed/external reader. Loopback addresses, matching paths, provider IDs, same UID or app type equality are **not** credentials. Independent publication/authorization needs #75/#60's separately accepted identity, grants and epoch/revocation semantics. Specialized ECS, graph, spatial, streaming, game and analytical domains are **not** required to adopt this record contract or implementation; they may share typed semantics with their domain-owned evaluator.

Memory and SQLite each own physical query execution, model-scoped visibility, bounded effort and cursor state. Shared typed predicate validation is a semantic conformance helper, not a universal execution engine. Existing Storage v2 applications are unchanged if they do not opt into Query. Delivery requires exact-head Linux validation, native provider and SDK tests, an architecture/compatibility review and normal protected-main integration; the evidence is linked from PR #177. Windows remains a manual pre-release check.
