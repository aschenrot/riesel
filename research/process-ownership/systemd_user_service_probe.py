#!/usr/bin/env python3
"""Research-only real systemd --user .service process-owner gate for Riesel #59.

Requires an operational systemd USER manager with cgroup v2. The fixture
creates only random, disposable riesel-research-59-*.service units belonging
to the executing account. It is not a Manager API or installer.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tempfile
import time
import unittest
import uuid

SELF = str(Path(__file__).resolve())
PREFIX = "riesel-research-59-"
PROPS = [
    "Id", "ActiveState", "SubState", "MainPID", "ExecMainPID",
    "InvocationID", "ControlGroup", "KillMode", "ExecStart", "Result",
]


def run(*argv: str, timeout: float = 12, check: bool = True, env=None):
    command = subprocess.run(
        list(argv), text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        timeout=timeout, check=False, env=env,
    )
    if check and command.returncode:
        raise AssertionError(
            f"command {argv!r} exited {command.returncode}: "
            f"stdout={command.stdout[-1500:]!r}, stderr={command.stderr[-1500:]!r}"
        )
    return command


def eventually(predicate, timeout: float = 8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    return predicate()


def proc_birth(pid: int):
    if not pid:
        return None
    try:
        rest = Path(f"/proc/{pid}/stat").read_text().split(") ", 1)[1].split()
        if rest[0] in ("Z", "X"):
            return None
        return rest[19]
    except (OSError, ValueError, IndexError):
        return None


def read_json(path: Path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def show(unit: str):
    argv = ["systemctl", "--user", "--no-pager", "show", unit]
    for prop in PROPS:
        argv.extend(["--property", prop])
    result = run(*argv)
    properties = {}
    for line in result.stdout.splitlines():
        key, separator, value = line.partition("=")
        if separator:
            properties[key] = value
    return properties


def owner_state(unit: str, marker: Path):
    state = show(unit)
    if state.get("ActiveState") != "active":
        return None
    current = read_json(marker)
    if not current or not read_json(marker.with_name(marker.name + ".child")):
        return None
    invocation = state.get("InvocationID", "")
    mainpid = int(state.get("MainPID", "0"))
    if not invocation or set(invocation) == {"0"} or mainpid != current["pid"]:
        return None
    if proc_birth(mainpid) is None:
        return None
    if current["invocation"].replace("-", "").lower() != invocation.replace("-", "").lower():
        return None
    if not state.get("ControlGroup", "").startswith("/"):
        return None
    if state.get("KillMode") != "control-group":
        return None
    return state, current, read_json(marker.with_name(marker.name + ".child"))


def start(unit: str, marker: Path, check: bool = True):
    if not re.fullmatch(r"riesel-research-59-[a-f0-9]{12}\.service", unit):
        raise AssertionError("refusing to control an unscoped unit")
    return run(
        "systemd-run", "--user", "--no-ask-password",
        "--unit", unit,
        "--property=Type=exec", "--property=KillMode=control-group",
        "--property=TimeoutStopSec=2s", "--property=Restart=no",
        sys.executable, SELF, "--app", str(marker),
        check=check,
    )


def stop_if_current(unit: str, expected_invocation: str):
    # This is a deliberately narrow *model* of generation-conditional
    # admission. systemctl --user stop does not itself provide a built-in
    # atomic expected-InvocationID check: a query/stop race remains.
    observed = show(unit)
    if observed.get("InvocationID") != expected_invocation:
        return "stale_refused"
    run("systemctl", "--user", "--no-pager", "stop", unit)
    return "stop_requested"


def child_mode(marker: Path):
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    marker.write_text(json.dumps({"pid": os.getpid(), "birth": proc_birth(os.getpid())}))
    while True:
        time.sleep(0.1)


def app_mode(marker: Path):
    # A descendant intentionally escapes the parent's POSIX session, but
    # inherits the service's cgroup. The child ignores SIGTERM to exercise
    # TimeoutStopSec escalation and cgroup cleanup.
    child = subprocess.Popen(
        [sys.executable, SELF, "--child", str(marker) + ".child"],
        start_new_session=True, stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    if not eventually(lambda: read_json(Path(str(marker) + ".child")), 3):
        raise RuntimeError("child did not initialize")

    def term(_signum, _frame):
        Path(str(marker) + ".stopped").write_text("cooperative")
        raise SystemExit(0)

    signal.signal(signal.SIGTERM, term)
    marker.write_text(json.dumps({
        "pid": os.getpid(),
        "birth": proc_birth(os.getpid()),
        "child_pid": child.pid,
        "invocation": os.environ.get("INVOCATION_ID", ""),
    }))
    while True:
        time.sleep(0.1)


def verify_real_manager():
    if not Path("/run/systemd/system").exists():
        raise RuntimeError("no booted systemd system manager")
    version = run("systemctl", "--version").stdout.splitlines()[0]
    runtime_dir = os.environ.get("XDG_RUNTIME_DIR", "")
    if not runtime_dir:
        raise RuntimeError("XDG_RUNTIME_DIR is absent")
    if not Path(runtime_dir, "systemd", "private").exists():
        raise RuntimeError("systemd user manager private socket absent")
    test = run("systemctl", "--user", "show-environment", timeout=7, check=False)
    if test.returncode:
        raise RuntimeError(
            f"operational systemd --user bus unavailable: {test.stderr[-700:]}"
        )
    print(f"REAL_SYSTEMD_USER_BUS_READY version={version!r} uid={os.getuid()}", flush=True)


@unittest.skipUnless(sys.platform == "linux", "Linux-specific actual systemd evidence")
class RealSystemdOwnerResearch(unittest.TestCase):
    def test_manager_crash_reconcile_restart_and_descendant_cleanup(self):
        # Fail rather than skip: lack of a systemd user-manager is an
        # environmental blocker, NOT a successful M1/M2 acceptance gate.
        verify_real_manager()
        with tempfile.TemporaryDirectory(prefix="riesel-sysd-59-") as root:
            marker = Path(root) / "owned.json"
            unit = PREFIX + uuid.uuid4().hex[:12] + ".service"
            tracked = []
            try:
                # Model a Manager dying with no reply immediately after the
                # operating-system manager accepts the start request.
                controller = run(
                    sys.executable, SELF, "--crash-controller", unit, str(marker),
                    check=False, timeout=15,
                )
                self.assertEqual(controller.returncode, 23, controller.stderr)
                first = eventually(lambda: owner_state(unit, marker))
                self.assertIsNotNone(first, "service not active after controller death")
                first_state, first_app, first_child = first
                tracked.extend((first_app, first_child))
                self.assertIn(str(marker), first_state.get("ExecStart", ""))
                self.assertEqual(first_state["Id"], unit)
                self.assertEqual(first_app["pid"], int(first_state["MainPID"]))
                self.assertIsNotNone(proc_birth(first_child["pid"]))
                initial_invocation = first_state["InvocationID"]
                print("RESEARCH_SYSTEMD_CONTROLLER_CRASH_SERVICE_SURVIVES", flush=True)

                # A second independently launched controller may observe
                # the same service owner without inheriting a Child handle.
                observer = run(sys.executable, SELF, "--observe", unit)
                observed = json.loads(observer.stdout)
                self.assertEqual(observed["InvocationID"], initial_invocation)
                self.assertEqual(int(observed["MainPID"]), first_app["pid"])
                self.assertEqual(observed["ControlGroup"], first_state["ControlGroup"])
                print("RESEARCH_SYSTEMD_FRESH_CONTROLLER_RECONCILES", flush=True)

                # A duplicate unit name must not cause a second live app.
                duplicate = start(unit, marker, check=False)
                self.assertNotEqual(duplicate.returncode, 0)
                self.assertEqual(show(unit)["InvocationID"], initial_invocation)
                print("RESEARCH_SYSTEMD_DUPLICATE_UNIT_REFUSED", flush=True)

                # Requesting restart from systemd creates a new OS attempt.
                # Model a registration retaining the unit name but requiring
                # the old invocation to be refused for subsequent control.
                run("systemctl", "--user", "--no-pager", "restart", unit)
                second = eventually(
                    lambda: (
                        value if (value := owner_state(unit, marker))
                        and value[0]["InvocationID"] != initial_invocation else None
                    ),
                    timeout=10,
                )
                self.assertIsNotNone(second, "systemd did not mint a new invocation")
                second_state, second_app, second_child = second
                tracked.extend((second_app, second_child))
                self.assertNotEqual(first_app["birth"], second_app["birth"])
                self.assertNotEqual(initial_invocation, second_state["InvocationID"])
                self.assertFalse(proc_birth(first_app["pid"]) == first_app["birth"])
                self.assertFalse(proc_birth(first_child["pid"]) == first_child["birth"])
                self.assertEqual(
                    stop_if_current(unit, initial_invocation), "stale_refused"
                )
                self.assertEqual(show(unit)["InvocationID"], second_state["InvocationID"])
                print("RESEARCH_SYSTEMD_STALE_INVOCATION_REFUSED_BY_MODEL", flush=True)

                self.assertEqual(
                    stop_if_current(unit, second_state["InvocationID"]), "stop_requested"
                )
                self.assertTrue(eventually(
                    lambda: show(unit).get("ActiveState") in ("inactive", "failed"),
                    timeout=10,
                ))
                self.assertTrue(eventually(
                    lambda: all(proc_birth(p["pid"]) != p["birth"] for p in tracked),
                    timeout=8,
                ), "residual descendants still alive after systemd unit stop")
                self.assertTrue(Path(str(marker) + ".stopped").exists())
                print("RESEARCH_SYSTEMD_CGROUP_DESCENDANTS_REAPED", flush=True)
                print("RESEARCH_REAL_SYSTEMD_USER_SERVICE_OWNER_PASS", flush=True)
            finally:
                # Cleanup only our random namespaced transient service. Do
                # not interact with any installed application or other units.
                try:
                    run("systemctl", "--user", "--no-pager", "stop", unit, check=False)
                finally:
                    run("systemctl", "--user", "--no-pager", "reset-failed", unit, check=False)


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "--child":
        child_mode(Path(sys.argv[2]))
    elif len(sys.argv) >= 2 and sys.argv[1] == "--app":
        app_mode(Path(sys.argv[2]))
    elif len(sys.argv) >= 2 and sys.argv[1] == "--crash-controller":
        outcome = start(sys.argv[2], Path(sys.argv[3]), check=False)
        if outcome.returncode:
            sys.stderr.write(outcome.stderr)
            sys.exit(88)
        os._exit(23)
    elif len(sys.argv) >= 2 and sys.argv[1] == "--observe":
        state = show(sys.argv[2])
        print(json.dumps({name: state.get(name) for name in PROPS}))
    else:
        unittest.main(verbosity=2)
