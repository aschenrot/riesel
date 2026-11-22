#!/usr/bin/env python3
"""Scratch-only SQLite live keyset consistency probe for Riesel #138.

No Riesel provider, user DB, production index, query API, or snapshot is used.
Run: python3 research/query-architecture/probes/sqlite_live_keyset.py
"""

import sqlite3
import unittest


class LiveKeysetProbe(unittest.TestCase):
    def setUp(self):
        self.db = sqlite3.connect(":memory:")
        self.db.execute("CREATE TABLE rows (id TEXT PRIMARY KEY, priority INTEGER NOT NULL)")
        self.db.executemany(
            "INSERT INTO rows(id, priority) VALUES (?, ?)",
            [("a", 1), ("b", 2), ("c", 3), ("d", 4)],
        )
        self.db.execute("CREATE INDEX by_priority_id ON rows(priority, id)")
        self.db.commit()

    def tearDown(self):
        self.db.close()

    def page(self, cursor=None, limit=1):
        if cursor is None:
            return self.db.execute(
                "SELECT id, priority FROM rows ORDER BY priority, id LIMIT ?",
                (limit,),
            ).fetchall()
        return self.db.execute(
            "SELECT id, priority FROM rows WHERE (priority, id) > (?, ?) "
            "ORDER BY priority, id LIMIT ?",
            (cursor[1], cursor[0], limit),
        ).fetchall()

    def drain(self, cursor):
        found = []
        while p := self.page(cursor):
            found.extend(id for id, _ in p)
            cursor = p[-1]
        return found

    def test_quiescent_keyset_is_complete(self):
        first = self.page()[0]
        self.assertEqual(first, ("a", 1))
        self.assertEqual([first[0]] + self.drain(first), ["a", "b", "c", "d"])

    def test_changed_sort_key_can_repeat_a_seen_record(self):
        first = self.page()[0]
        self.db.execute("UPDATE rows SET priority=5 WHERE id='a'")
        self.db.commit()
        result = [first[0]] + self.drain(first)
        self.assertEqual(result, ["a", "b", "c", "d", "a"])
        self.assertEqual(len(result), 5)

    def test_changed_sort_key_can_hide_an_unseen_record(self):
        first = self.page()[0]
        self.db.execute("UPDATE rows SET priority=0 WHERE id='d'")
        self.db.commit()
        result = [first[0]] + self.drain(first)
        self.assertEqual(result, ["a", "b", "c"])
        self.assertNotIn("d", result)


if __name__ == "__main__":
    print(f"Python sqlite3 SQLite version: {sqlite3.sqlite_version}")
    unittest.main(verbosity=2)
