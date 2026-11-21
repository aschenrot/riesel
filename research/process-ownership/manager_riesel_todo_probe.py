#!/usr/bin/env python3
"""#59 research: real Riesel Todo HTTP hosted by a transient systemd user unit.

Tests observable callable readiness, controller-crash survival, app-specific
SIGINT-based graceful stop and persistent data across distinct OS attempts.
No Manager API, installer or authorization guarantees are implemented.
"""
from __future__ import annotations

import json
from pathlib import Path
import os
import re
import subprocess
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request
import uuid

import systemd_user_service_probe as owner

ROOT = Path(__file__).resolve().parents[2]
TODO = ROOT / "target" / "debug" / "todo_http"
READY = re.compile(r"^TODO_HTTP_READY (127\.0\.0\.1:[0-9]+)$")


def unit_for_attempt():
    return owner.PREFIX + uuid.uuid4().hex[:12] + ".service"


def require_research_unit(unit: str):
    if not re.fullmatch(r"riesel-research-59-[a-f0-9]{12}\.service", unit):
        raise AssertionError("refuse non-research unit")


def start_real_app(unit: str, db: Path, log: Path):
    require_research_unit(unit)
    return owner.run(
        "systemd-run", "--user", "--no-ask-password", "--unit", unit,
        "--property=Type=exec",
        "--property=KillMode=control-group",
        "--property=Restart=no",
        "--property=TimeoutStopSec=7s",
        # Todo's accepted local HTTP adapter listens to Ctrl-C/SIGINT.
        # A generic Riesel Manager must NOT assume this for other apps.
        "--property=KillSignal=SIGINT",
        "--property=StandardOutput=append:" + str(log),
        "--property=StandardError=append:" + str(log),
        str(TODO), "--db", str(db), "--port", "0",
    )


def request(url: str, operation="GET", body=None):
    headers = {}
    if body is not None:
        headers = {
            "Content-Type": "application/json",
            "Origin": url.split("/api/", 1)[0],
            "Sec-Fetch-Site": "same-origin",
        }
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(
        url, data=data, headers=headers, method=operation
    )
    with urllib.request.urlopen(req, timeout=3) as response:
        return response.status, json.loads(response.read())


def ready_endpoint(unit: str, logfile: Path):
    # A systemd ActiveState alone MUST NOT count as a usable app interface.
    if logfile.exists():
        for line in logfile.read_text(errors="replace").splitlines():
            found = READY.fullmatch(line.strip())
            if found:
                endpoint = "http://" + found.group(1)
                try:
                    status, body = request(endpoint + "/api/v1/tasks")
                except (OSError, ValueError, urllib.error.URLError):
                    return None
                if status == 200 and isinstance(body.get("records"), list):
                    return endpoint
    return None


def wait_stop(unit, pid, birth, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if owner.proc_birth(pid) != birth:
            return True
        time.sleep(0.05)
    return owner.proc_birth(pid) != birth


def exit_details(unit: str):
    props = ("ExecMainCode", "ExecMainStatus", "Result", "ActiveState")
    cmd = ["systemctl", "--user", "--no-pager", "show", unit]
    for key in props:
        cmd.extend(("--property", key))
    data = owner.run(*cmd, check=False)
    result = {}
    for line in data.stdout.splitlines():
        name, equal, value = line.partition("=")
        if equal:
            result[name] = value
    return result


@unittest.skipUnless(sys.platform == "linux", "actual Linux user manager only")
class RealRieselServiceOwnerProbe(unittest.TestCase):
    def test_restarted_manager_and_graceful_riesel_todo(self):
        owner.verify_real_manager()
        self.assertTrue(TODO.is_file(), f"build real Todo HTTP first: {TODO}")
        with tempfile.TemporaryDirectory(prefix="riesel-59-real-todo-") as tmp:
            root = Path(tmp)
            db = root / "persistent.db"
            first, second = unit_for_attempt(), unit_for_attempt()
            log_first, log_second = root / "attempt-a.log", root / "attempt-b.log"
            try:
                # The initiating controller dies abruptly after OS admission.
                controller = subprocess.run(
                    [sys.executable, __file__, "--launch-and-crash",
                     first, str(db), str(log_first)],
                    timeout=15, capture_output=True, text=True,
                )
                self.assertEqual(controller.returncode, 23, controller.stderr)
                endpoint_a = owner.eventually(
                    lambda: ready_endpoint(first, log_first), timeout=15
                )
                self.assertIsNotNone(endpoint_a, "real app did not become HTTP-callable")
                os_a = owner.show(first)
                pid_a = int(os_a["MainPID"])
                birth_a = owner.proc_birth(pid_a)
                self.assertIsNotNone(birth_a)
                self.assertEqual(os_a["ActiveState"], "active")
                self.assertTrue(os_a["InvocationID"])
                self.assertIn(str(TODO), os_a["ExecStart"])
                print("RESEARCH_MANAGER_REAL_RIESEL_APP_SURVIVES_CONTROLLER_CRASH", flush=True)
                print("RESEARCH_MANAGER_OS_ACTIVE_AND_HTTP_CALLABLE_CHECKED_SEPARATELY", flush=True)

                status, created = request(
                    endpoint_a + "/api/v1/tasks",
                    "POST", {"title": "Manager-owned retained Todo"},
                )
                self.assertEqual(status, 201)
                task_id = created["id"]
                self.assertEqual(created["title"], "Manager-owned retained Todo")

                # A new independent controller can stop the OS unit using
                # the app's explicitly supported SIGINT, not generic SIGTERM.
                stopped = subprocess.run(
                    [sys.executable, __file__, "--stop", first],
                    capture_output=True, text=True, timeout=15,
                )
                self.assertEqual(stopped.returncode, 0, stopped.stderr)
                self.assertTrue(wait_stop(first, pid_a, birth_a))
                details = exit_details(first)
                print("RESEARCH_MANAGER_REAL_RIESEL_TODO_EXIT", json.dumps(details), flush=True)
                # An application-cooperative stop should exit normally (not
                # merely be classified as a clean systemd SIGTERM signal).
                self.assertEqual(details.get("ExecMainStatus"), "0", details)
                self.assertEqual(details.get("ExecMainCode"), "1", details)
                print("RESEARCH_MANAGER_REAL_RIESEL_APP_GRACEFUL_SIGINT_STOP", flush=True)

                # A new OS attempt owns a unique immutable unit name and
                # *reuses the same explicit* SQLite store without replacing it.
                start_real_app(second, db, log_second)
                endpoint_b = owner.eventually(
                    lambda: ready_endpoint(second, log_second), timeout=15
                )
                self.assertIsNotNone(endpoint_b)
                os_b = owner.show(second)
                self.assertNotEqual(os_a["InvocationID"], os_b["InvocationID"])
                self.assertNotEqual(os_a["Id"], os_b["Id"])
                status, persisted = request(
                    endpoint_b + "/api/v1/tasks/" + task_id
                )
                self.assertEqual(status, 200)
                self.assertEqual(persisted["title"], "Manager-owned retained Todo")
                print("RESEARCH_MANAGER_REAL_RIESEL_SQLITE_PERSISTS_AFTER_OS_OWNER_CHANGE",
                      flush=True)

                # Stale operations for prior immutable OS owner do not name B.
                owner.run("systemctl", "--user", "--no-pager",
                          "stop", first, check=False)
                self.assertEqual(owner.show(second)["InvocationID"],
                                 os_b["InvocationID"])
                self.assertEqual(request(endpoint_b + "/api/v1/tasks")[0], 200)
                print("RESEARCH_MANAGER_OLD_ATTEMPT_STOP_CANNOT_NAME_NEW_TODO_SERVICE",
                      flush=True)

                owner.run("systemctl", "--user", "--no-pager", "stop", second)
                print("RESEARCH_MANAGER_REAL_RIESEL_TODO_OWNER_PASS", flush=True)
            finally:
                for unit in (first, second):
                    owner.run("systemctl", "--user", "--no-pager", "stop",
                              unit, check=False)
                    owner.run("systemctl", "--user", "--no-pager",
                              "reset-failed", unit, check=False)


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "--launch-and-crash":
        start_real_app(sys.argv[2], Path(sys.argv[3]), Path(sys.argv[4]))
        os._exit(23)
    elif len(sys.argv) >= 2 and sys.argv[1] == "--stop":
        require_research_unit(sys.argv[2])
        owner.run("systemctl", "--user", "--no-pager", "stop", sys.argv[2])
    else:
        unittest.main(verbosity=2)
