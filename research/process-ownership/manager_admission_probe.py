#!/usr/bin/env python3
"""Research only: durable registration admission + real systemd attempt ownership.

This is a deliberately bounded comparator for issue #59. SQLite and systemd
cannot participate in one atomic transaction. The experiment illustrates
reservation/reconciliation under cooperating trusted same-UID controllers,
NOT production authorization, a public Manager or a general exactly-once API.

All managed units are random, disposable 'riesel-research-59-*.service' user
services; the existing real-systemd fixture owns their OS-level behavior.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

import systemd_user_service_probe as os_owner

SELF = str(Path(__file__).resolve())
PHASES = {"reserved", "active", "stopping", "stopped", "unknown"}


def db_connection(path: str):
    db = sqlite3.connect(path, timeout=8, isolation_level=None)
    db.execute("PRAGMA busy_timeout=8000")
    return db


def init_db(path: str):
    with db_connection(path) as db:
        db.execute("PRAGMA journal_mode=WAL")
        db.executescript("""
            CREATE TABLE registrations (
                id TEXT PRIMARY KEY,
                current_attempt TEXT NOT NULL
            );
            CREATE TABLE attempts (
                id TEXT PRIMARY KEY,
                registration TEXT NOT NULL,
                unit TEXT NOT NULL UNIQUE,
                marker TEXT NOT NULL,
                phase TEXT NOT NULL,
                invocation TEXT,
                FOREIGN KEY (registration) REFERENCES registrations(id)
            );
        """)


def record(db, registration: str):
    row = db.execute(
        """SELECT a.id,a.unit,a.marker,a.phase,a.invocation
           FROM registrations r JOIN attempts a
             ON a.id=r.current_attempt
           WHERE r.id=?""",
        (registration,),
    ).fetchone()
    if row is None:
        return None
    return dict(zip(("attempt", "unit", "marker", "phase", "invocation"), row))


def claim(path: str, registration: str, marker_dir: str):
    # Durable, unique unit name/attempt reservation BEFORE asking systemd
    # to launch. Transactions serialize cooperating, separate controllers.
    attempt = uuid.uuid4().hex
    unit = os_owner.PREFIX + attempt[:12] + ".service"
    marker = str(Path(marker_dir) / ("app-" + attempt))
    with db_connection(path) as db:
        db.execute("BEGIN IMMEDIATE")
        before = record(db, registration)
        if before is not None and before["phase"] != "stopped":
            db.execute("ROLLBACK")
            return {"outcome": "denied", "phase": before["phase"],
                    "current_attempt": before["attempt"]}
        db.execute(
            "INSERT INTO attempts(id,registration,unit,marker,phase) VALUES(?,?,?,?,?)",
            (attempt, registration, unit, marker, "reserved"),
        )
        if before is None:
            db.execute(
                "INSERT INTO registrations(id,current_attempt) VALUES(?,?)",
                (registration, attempt),
            )
        else:
            db.execute(
                "UPDATE registrations SET current_attempt=? WHERE id=?",
                (attempt, registration),
            )
        db.execute("COMMIT")
    return {"outcome": "reserved", "attempt": attempt, "unit": unit,
            "marker": marker}


def snapshot(path, registration):
    with db_connection(path) as db:
        return record(db, registration)


def os_evidence(info):
    # This app-authored evidence is NOT cryptographically authenticated,
    # and is insufficient against an untrusted same-UID peer.
    return os_owner.owner_state(info["unit"], Path(info["marker"]))


def reconcile(path: str, registration: str, expected_attempt: str):
    with db_connection(path) as db:
        db.execute("BEGIN IMMEDIATE")
        info = record(db, registration)
        if info is None or info["attempt"] != expected_attempt:
            db.execute("ROLLBACK")
            return "stale_refused"
        if info["phase"] in ("reserved", "unknown", "active"):
            evidence = os_owner.eventually(lambda: os_evidence(info), timeout=4)
            if not evidence:
                db.execute("ROLLBACK")
                return "unknown"
            state, _app, _child = evidence
            if info["invocation"] and info["invocation"] != state["InvocationID"]:
                db.execute("ROLLBACK")
                return "invocation_mismatch"
            db.execute(
                """UPDATE attempts SET phase='active',invocation=?
                   WHERE id=? AND phase IN ('reserved','unknown','active')""",
                (state["InvocationID"], expected_attempt),
            )
            db.execute("COMMIT")
            return "active"
        if info["phase"] == "stopping":
            try:
                state = os_owner.show(info["unit"])
                is_inactive = state.get("ActiveState") in ("inactive", "failed")
            except AssertionError:
                is_inactive = False
            initial = os_owner.read_json(Path(info["marker"]))
            child = os_owner.read_json(Path(info["marker"] + ".child"))
            pids_gone = bool(initial and child) and all(
                os_owner.proc_birth(p["pid"]) != p["birth"]
                for p in (initial, child)
            )
            if not (is_inactive and pids_gone):
                db.execute("ROLLBACK")
                return "unknown"
            db.execute(
                "UPDATE attempts SET phase='stopped' WHERE id=? AND phase='stopping'",
                (expected_attempt,),
            )
            db.execute("COMMIT")
            return "stopped"
        db.execute("ROLLBACK")
        return info["phase"]


def begin_stop(path, registration: str, expected_attempt: str,
               expected_invocation: str):
    # Unique unit per attempt prevents the resulting systemd stop from
    # addressing another attempt's unit, even if this process crashes.
    with db_connection(path) as db:
        db.execute("BEGIN IMMEDIATE")
        info = record(db, registration)
        if (info is None or info["attempt"] != expected_attempt
                or info["phase"] != "active"
                or info["invocation"] != expected_invocation):
            db.execute("ROLLBACK")
            return "stale_refused"
        db.execute(
            "UPDATE attempts SET phase='stopping' WHERE id=? AND phase='active'",
            (expected_attempt,),
        )
        db.execute("COMMIT")
        return "stopping"


def stop_os_unit(unit: str):
    # The name is immutable for this attempt, never shared across attempts.
    if not unit.startswith(os_owner.PREFIX) or not unit.endswith(".service"):
        raise AssertionError("refusing an OS stop outside the research namespace")
    os_owner.run("systemctl", "--user", "--no-pager", "stop", unit)


def child_command(*args):
    child = subprocess.run(
        [sys.executable, SELF, *args],
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        timeout=18, check=False,
    )
    return child


@unittest.skipUnless(sys.platform == "linux", "real systemd Linux user bus")
class DurableRegistrationResearch(unittest.TestCase):
    def setUp(self):
        os_owner.verify_real_manager()
        self.tmp = tempfile.TemporaryDirectory(prefix="riesel-mgr-registry-59-")
        self.root = Path(self.tmp.name)
        self.db = str(self.root / "registry.db")
        init_db(self.db)
        self.units = set()
        self.registration = "registered-counter"

    def tearDown(self):
        for unit in sorted(self.units):
            os_owner.run("systemctl", "--user", "--no-pager", "stop", unit,
                         check=False)
            os_owner.run("systemctl", "--user", "--no-pager", "reset-failed",
                         unit, check=False)
        self.tmp.cleanup()

    def reserve(self):
        info = claim(self.db, self.registration, str(self.root))
        if info["outcome"] == "reserved":
            self.units.add(info["unit"])
        return info

    def running(self, info, crash_after_start=False):
        if crash_after_start:
            child = child_command("--crash-after-start", info["unit"],
                                  info["marker"])
            self.assertEqual(child.returncode, 23, child.stderr)
        else:
            os_owner.start(info["unit"], Path(info["marker"]))
        state = os_owner.eventually(lambda: os_evidence(info), timeout=8)
        self.assertIsNotNone(state)
        return state

    def test_01_concurrent_controller_claim_and_start_crash_reconcile(self):
        # Distinct processes actually race on the same SQLite registration.
        workers = [
            subprocess.Popen(
                [sys.executable, SELF, "--claim", self.db,
                 self.registration, str(self.root)],
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            ) for _ in range(4)
        ]
        replies = []
        for worker in workers:
            output, errors = worker.communicate(timeout=10)
            self.assertEqual(worker.returncode, 0, errors)
            replies.append(json.loads(output.strip()))
        winners = [x for x in replies if x["outcome"] == "reserved"]
        self.assertEqual(len(winners), 1, replies)
        winner = winners[0]
        self.units.add(winner["unit"])
        self.assertEqual(snapshot(self.db, self.registration)["attempt"],
                         winner["attempt"])
        print("RESEARCH_MANAGER_SQLITE_CONCURRENT_SINGLE_ADMISSION", flush=True)

        self.running(winner, crash_after_start=True)
        # Controller failed after OS start but BEFORE recording outcome.
        current = snapshot(self.db, self.registration)
        self.assertEqual(current["phase"], "reserved")
        self.assertEqual(self.reserve()["outcome"], "denied")
        self.assertEqual(
            reconcile(self.db, self.registration, winner["attempt"]), "active"
        )
        self.assertEqual(snapshot(self.db, self.registration)["phase"], "active")
        print("RESEARCH_MANAGER_CRASH_AFTER_OS_START_RECONCILED", flush=True)

    def test_02_per_attempt_stale_stop_and_crash_after_os_stop(self):
        first = self.reserve()
        self.assertEqual(first["outcome"], "reserved")
        self.running(first)
        self.assertEqual(reconcile(self.db, self.registration,
                                   first["attempt"]), "active")
        a = snapshot(self.db, self.registration)
        self.assertEqual(begin_stop(self.db, self.registration, a["attempt"],
                                    a["invocation"]), "stopping")
        self.assertEqual(self.reserve()["outcome"], "denied")
        stop_os_unit(a["unit"])
        self.assertEqual(reconcile(self.db, self.registration,
                                   a["attempt"]), "stopped")

        second = self.reserve()
        self.assertEqual(second["outcome"], "reserved")
        self.running(second)
        self.assertEqual(reconcile(self.db, self.registration,
                                   second["attempt"]), "active")
        b = snapshot(self.db, self.registration)
        # An old stop cannot address the new app by stale registration token.
        self.assertEqual(begin_stop(self.db, self.registration, a["attempt"],
                                    a["invocation"]), "stale_refused")
        self.assertEqual(os_evidence(b)[0]["InvocationID"], b["invocation"])
        # And even an out-of-band stop of old immutable unit name does not
        # address B's separately owned service.
        os_owner.run("systemctl", "--user", "--no-pager", "stop", a["unit"],
                     check=False)
        self.assertEqual(os_evidence(b)[0]["InvocationID"], b["invocation"])
        print("RESEARCH_MANAGER_DURABLE_ATTEMPT_FENCES_STALE_STOP", flush=True)

        self.assertEqual(begin_stop(self.db, self.registration, b["attempt"],
                                    b["invocation"]), "stopping")
        self.assertEqual(self.reserve()["outcome"], "denied")
        child = child_command("--crash-after-stop", b["unit"])
        self.assertEqual(child.returncode, 23, child.stderr)
        self.assertEqual(snapshot(self.db, self.registration)["phase"], "stopping")
        self.assertEqual(reconcile(self.db, self.registration,
                                   b["attempt"]), "stopped")
        print("RESEARCH_MANAGER_CRASH_AFTER_OS_STOP_RECONCILED", flush=True)

    def test_03_reserved_no_os_start_is_not_silently_replayed(self):
        attempt = self.reserve()
        self.assertEqual(attempt["outcome"], "reserved")
        # Simulate crash between persisted reservation and systemd call.
        self.assertEqual(reconcile(self.db, self.registration,
                                   attempt["attempt"]), "unknown")
        self.assertEqual(self.reserve()["outcome"], "denied")
        self.assertEqual(snapshot(self.db, self.registration)["phase"], "reserved")
        # A later operator can reconcile explicitly; no implicit new
        # attempt or false claim of successful start.
        print("RESEARCH_MANAGER_UNCERTAIN_RESERVATION_FAILS_CLOSED", flush=True)

    def test_04_same_uid_registry_is_not_strong_caller_auth(self):
        attempt = self.reserve()
        self.assertEqual(attempt["outcome"], "reserved")
        os.chmod(self.db, 0o600)
        # A distinct untrusted process under the *same UID* reads the
        # registry. File mode is not a principal boundary here.
        sibling = child_command("--read-registry", self.db, self.registration)
        self.assertEqual(sibling.returncode, 0, sibling.stderr)
        self.assertEqual(json.loads(sibling.stdout)["attempt"],
                         attempt["attempt"])
        print("RESEARCH_MANAGER_SAME_UID_REGISTRY_IS_NOT_AUTH", flush=True)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--claim":
        print(json.dumps(claim(sys.argv[2], sys.argv[3], sys.argv[4])),
              flush=True)
    elif len(sys.argv) > 1 and sys.argv[1] == "--crash-after-start":
        os_owner.start(sys.argv[2], Path(sys.argv[3]))
        os._exit(23)
    elif len(sys.argv) > 1 and sys.argv[1] == "--crash-after-stop":
        stop_os_unit(sys.argv[2])
        os._exit(23)
    elif len(sys.argv) > 1 and sys.argv[1] == "--read-registry":
        print(json.dumps(snapshot(sys.argv[2], sys.argv[3])), flush=True)
    else:
        unittest.main(verbosity=2)
