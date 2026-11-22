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
