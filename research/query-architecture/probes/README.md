# Scratch-only keyset pagination research

This reproducible research fixture is **not** a Riesel provider or an
implementation of any accepted query capability.

Run from the repository root with Python 3.10+ and a system SQLite build
that supports row-value comparisons:

```sh
python3 research/query-architecture/probes/sqlite_live_keyset.py
```

It creates only an in-memory SQLite table/index and demonstrates:
(1) stable keyset reads when data does not change; (2) duplicate rows
after a previously seen row moves forward in sort order; and
(3) omitted rows after an unseen row moves behind a cursor.
The original local run used Python `sqlite3` with SQLite 3.46.1
and **3 tests passed**. GitHub Actions runs the same fixture separately.
No concurrency or isolation-level matrix was tested: the mutation
happens deterministically **between** page queries, not inside one
long-running snapshot transaction. The tests do not demonstrate
an error in Riesel `RecordStore` v2, which sorts by UUID and
makes no multi-page snapshot guarantee.

Compare the existing [Todo #107 experiments](https://github.com/aschenrot/riesel/pull/130)
rather than duplicating them. See [REPORT.md](../REPORT.md) for interpretation.

## Typed record predicate and cursor research

Run **cargo test -p todo --test query_architecture_research -- --nocapture** from the workspace root with Rust 1.100.0. This test wrapper loads the fixture in **typed_record_contract.rs**. It registers one derived test model on the real Memory and linked SQLite providers and exercises a bounded **research-only** predicate interpreter over Storage v2 UUID pages. It checks full-width numeric roundtrip, optional null/missing distinctions, rejected unsupported predicates, explicit empty partial results and test-local cursor model/filter/generation checks. [Research CI #44968670157](https://github.com/aschenrot/riesel/actions/runs/44968670157) passed both Rust tests at earlier revision **a3aa116c**.

It does **not** test provider-native index use, managed sessions, security or authentication, snapshot guarantees, a public Query capability, or a latency SLA.

## Exact SQLite expression / bounded-execution research

Run `cargo test -p riesel_storage_sqlite --test query_architecture_research -- --nocapture` from the root (pinned Rust 1.100.0, existing `rusqlite 0.43.0` linked to bundled SQLite). The wrapper at `packages/storage/sqlite/tests/query_architecture_research.rs` imports `sqlite_native_predicates.rs`. The fixture **first uses the real SQLite RecordStore v2 provider to write valid registered records**, drops that provider, then uses direct SQLite on a disposable temporary DB. This respects the research's no-live-DB-index rule but **is not** a provider-owned capability or authorized remote client.

The **four** native SQL tests check that JSON `->` preserves exact full-width integer tokens, `json_extract` can round large `u64` values, `json_type` distinguishes missing and JSON null, **string ordering is not numeric ordering**, equivalent uppercase/lowercase UUIDs differ under naive SQL text equality, a **forced** scratch expression index realizes exact equality, and `progress_handler` can interrupt one read without corrupting the connection. Passing CI neither proves automatic index selection nor production index upgrades, general SQL cancellation, secure access, native Memory parity or Query API acceptance. [Research CI #44971511998](https://github.com/aschenrot/riesel/actions/runs/44971511998) passed the four SQL tests on commit `2404ec84` with bundled SQLite **3.54.1**, plus two Rust reference tests and three separate Python SQLite tests. Use newer exact-head CI after revisions.

## Sparse query work cost and cancellation budget (independent scratch experiment)

Run `python3 research/query-architecture/probes/sqlite_cost_budget.py`. This Python stdlib experiment constructs a disposable in-memory SQLite table with UUID-ordered models at 100/10,000 records and only the final record matching the boolean predicate; the index is created **only** in the scratch database. The test counts one progress callback per **50 SQLite VDBE instructions**, prints `EXPLAIN QUERY PLAN` before/after the expression index and verifies identical matches, budget interruption and predicate-index update maintenance. [CI #44972111565](https://github.com/aschenrot/riesel/actions/runs/44972111565) passed at commit `dc8a4a4` (SQLite 3.46.1). At 100 rows: 12 scan callbacks versus zero indexed; at 10,000: 1,200 scan callbacks versus zero indexed. Zero callbacks means fewer than 50 instructions, **not zero work**. These are deterministic instruction-work proxies, not production latency benchmarks, a provider migration, remote cancellation, or a public Query capability.
