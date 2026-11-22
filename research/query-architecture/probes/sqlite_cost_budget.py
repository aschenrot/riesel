#!/usr/bin/env python3
"""#138 scratch-only SQLite query-cost / interruption counterexamples.

Reproduce: python3 research/query-architecture/probes/sqlite_cost_budget.py
Data is synthetic; never opens or modifies a Riesel provider database.
Progress-handler callback counts proxy VM work, NOT time/latency or CPU.
"""
import sqlite3
import unittest

STEP_INTERVAL = 50
MODEL = "research"
QUERY = (
    "SELECT id FROM records WHERE model=? "
    "AND json_type(CAST(data AS TEXT),'$.completed')='false' "
    "ORDER BY id LIMIT 32"
)
INDEX_SQL = (
    "CREATE INDEX research_status_idx ON records("
    "model, json_type(CAST(data AS TEXT),'$.completed'), id)"
)

def dataset(size):
    conn = sqlite3.connect(":memory:")
    conn.execute(
        "CREATE TABLE records (model TEXT NOT NULL, id BLOB NOT NULL, "
        "data BLOB NOT NULL, PRIMARY KEY(model,id)) WITHOUT ROWID"
    )
    conn.executemany(
        "INSERT INTO records(model,id,data) VALUES (?,?,?)",
        (
            (MODEL, index.to_bytes(16, "big"),
             '{"completed":false}' if index == size - 1
             else '{"completed":true}')
            for index in range(size)
        ),
    )
    return conn

def measure(conn, limit_callbacks=None):
    callbacks = [0]
    def progress():
        callbacks[0] += 1
        return int(limit_callbacks is not None and callbacks[0] >= limit_callbacks)
    conn.set_progress_handler(progress, STEP_INTERVAL)
    try:
        values = conn.execute(QUERY, (MODEL,)).fetchall()
        return callbacks[0], [int.from_bytes(row[0], "big") for row in values], None
    except sqlite3.OperationalError as exc:
        if "interrupt" not in str(exc).lower():
            raise
        return callbacks[0], [], str(exc)
    finally:
        conn.set_progress_handler(None, 0)

def plan(conn):
    return [str(row[3]) for row in conn.execute("EXPLAIN QUERY PLAN " + QUERY, (MODEL,))]

class SqliteCostEvidence(unittest.TestCase):
    def test_indexed_sparse_match_avoids_full_scan_work_at_100_and_10000(self):
        for size in (100, 10_000):
            with self.subTest(size=size):
                conn = dataset(size)
                try:
                    before = plan(conn)
                    scan_ticks, scan_result, error = measure(conn)
                    self.assertIsNone(error)
                    self.assertEqual(scan_result, [size - 1])
                    conn.execute(INDEX_SQL)
                    after = plan(conn)
                    index_ticks, index_result, error = measure(conn)
                    self.assertIsNone(error)
                    self.assertEqual(index_result, scan_result)
                    self.assertTrue(any("research_status_idx" in p for p in after), after)
                    self.assertTrue(all("research_status_idx" not in p for p in before), before)
                    # Not a throughput benchmark: instruction-count proxy only.
                    self.assertGreater(scan_ticks, index_ticks * 5 + 2)
                    print(
                        f"size={size} SQLite={sqlite3.sqlite_version} "
                        f"scan_callbacks={scan_ticks} indexed_callbacks={index_ticks} "
                        f"interval={STEP_INTERVAL} "
                        f"scan_plan={before} indexed_plan={after}"
                    )
                finally:
                    conn.close()

    def test_budget_rejects_unindexed_scan_but_allows_indexed_lookup(self):
        conn = dataset(10_000)
        try:
            ticks, results, error = measure(conn, limit_callbacks=30)
            self.assertEqual(ticks, 30)
            self.assertEqual(results, [])
            self.assertIsNotNone(error)
            conn.execute(INDEX_SQL)
            indexed_ticks, indexed_results, error = measure(conn, limit_callbacks=30)
            self.assertIsNone(error)
            self.assertEqual(indexed_results, [9_999])
            self.assertLess(indexed_ticks, 30)
        finally:
            conn.close()

    def test_scratch_expression_index_tracks_committed_predicate_updates(self):
        conn = dataset(100)
        try:
            conn.execute(INDEX_SQL)
            self.assertEqual(measure(conn)[1], [99])
            conn.execute(
                "UPDATE records SET data=? WHERE model=? AND id=?",
                ('{"completed":true}', MODEL, (99).to_bytes(16,"big")),
            )
            self.assertEqual(measure(conn)[1], [])
            conn.execute(
                "UPDATE records SET data=? WHERE model=? AND id=?",
                ('{"completed":false}', MODEL, (2).to_bytes(16,"big")),
            )
            self.assertEqual(measure(conn)[1], [2])
        finally:
            conn.close()

if __name__ == "__main__":
    print(f"Python sqlite3 SQLite version: {sqlite3.sqlite_version}")
    unittest.main(verbosity=2)
