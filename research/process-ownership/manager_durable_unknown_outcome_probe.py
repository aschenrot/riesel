#!/usr/bin/env python3
"""#59/#75 research: crash-window reconciliation on a REAL OS-owned Riesel app.

Two independently spawned root *test* workers are killed with os._exit
immediately before, or after, a systemd stop. SQLite FULL journal survives
these controller process deaths; a new root observer inspects Linux OS state
and never retries a possibly executed mutation. A restricted same-UID
adversary is unable to edit the protected journal or stop the app directly.

This is a bounded research fixture, NOT production Manager or a proof of
trusted human approval, stable power-loss durability, atomic stop, or parity.
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import signal
import sqlite3
import stat
import subprocess
import sys
import tempfile
import time
import uuid

PINNED = "5201e61cc74bab778dfc63a3788f1708b2530f21"
UNTRUSTED = 60090
PFX = "riesel-m59-unknown-"
DIR = "/opt/riesel-m59-unknown-"
ROOT = "riesel-m59-unknown-journal-"
EXIT_BEFORE = 73
EXIT_AFTER = 77


def run(args, *, check=True, timeout=20):
    p = subprocess.run([str(a) for a in args], cwd="/tmp",
                       stdin=subprocess.DEVNULL, text=True,
                       capture_output=True, timeout=timeout, check=False)
    if check and p.returncode:
        raise AssertionError(
            f"command failed rc={p.returncode} argv={args[:4]!r} "
            f"err={p.stderr[-700:]!r} out={p.stdout[-300:]!r}"
        )
    return p


def elevated(*args):
    return ["sudo", "-n", *args]


def valid_unit(unit):
    if not re.fullmatch(r"riesel-m59-unknown-[a-f0-9]{12}\.service", unit):
        raise AssertionError("refusing nonresearch systemd unit")
    return unit


def valid_app(path):
    if not re.fullmatch(r"/opt/riesel-m59-unknown-[a-f0-9]{12}", str(path)):
        raise AssertionError("refusing nonresearch installed path")
    return path


def valid_root(root):
    if not re.fullmatch(r"riesel-m59-unknown-journal-[a-zA-Z0-9_-]+",
                        root.name) or root.parent != Path("/tmp"):
        raise AssertionError("refusing nonresearch journal directory")
    return root


def os_state(unit):
    valid_unit(unit)
    p = run(["systemctl", "--system", "--no-pager", "--no-ask-password",
             "show", unit, "--property=MainPID", "--property=InvocationID",
             "--property=ActiveState", "--property=Result"])
    return dict(x.split("=", 1) for x in p.stdout.splitlines() if "=" in x)


def start_tick(pid):
    raw = Path(f"/proc/{pid}/stat").read_text()
    return int(raw[raw.rfind(")") + 2:].split()[19])


def live_exact(pid, tick):
    try:
        return start_tick(pid) == tick
    except (OSError, ProcessLookupError):
        return False


def counter_pid(executable):
    result = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            binary = (entry / "cmdline").read_bytes().split(b"\0", 1)[0]
        except (OSError, ProcessLookupError):
            continue
        if binary == os.fsencode(executable):
            result.append(int(entry.name))
    if len(result) != 1:
        raise AssertionError(f"expected exactly one live real Counter: {result}")
    return result[0]


def observe_running(unit, executable, *, previous=None):
    for _ in range(100):
        state = os_state(unit)
        if state.get("ActiveState") == "active" and state.get("InvocationID"):
            try:
                pid = counter_pid(executable)
                tick = start_tick(pid)
            except (AssertionError, OSError, ProcessLookupError):
                time.sleep(0.2)
                continue
            if (previous is None or state["InvocationID"] != previous) and (
                live_exact(pid, tick)
            ):
                return {"unit": unit, "invocation": state["InvocationID"],
                        "pid": pid, "tick": tick}
        time.sleep(0.2)
    raise AssertionError(f"real Counter attempt not observed: {os_state(unit)}")


def protected(root):
    valid_root(root)
    st = root.lstat()
    if (not stat.S_ISDIR(st.st_mode) or st.st_uid != 0 or
            st.st_mode & 0o077):
        raise AssertionError("research journal must be root-owned 0700")
    db = root / "commands.sqlite"
    if db.exists():
        fst = db.lstat()
        if (not stat.S_ISREG(fst.st_mode) or fst.st_uid != 0 or
                fst.st_mode & 0o077):
            raise AssertionError("research SQLite journal not root-owned 0600")
    return db


def connection(root):
    if os.geteuid() != 0:
        raise AssertionError("only protected root test worker accesses journal")
    os.umask(0o077)
    path = protected(root)
    db = sqlite3.connect(path, timeout=5, isolation_level=None)
    db.execute("PRAGMA synchronous=FULL")
    db.execute("PRAGMA journal_mode=DELETE")
    db.execute("""CREATE TABLE IF NOT EXISTS commands (
        id TEXT PRIMARY KEY, unit TEXT NOT NULL, invocation TEXT NOT NULL,
        pid INTEGER NOT NULL, start_tick INTEGER NOT NULL,
        executable_sha256 TEXT NOT NULL, verb TEXT NOT NULL,
        phase TEXT NOT NULL CHECK(phase = 'submitted')
    )""")
    db.execute("""CREATE TABLE IF NOT EXISTS observations (
        seq INTEGER PRIMARY KEY AUTOINCREMENT,
        command_id TEXT NOT NULL, verdict TEXT NOT NULL,
        unit_state TEXT NOT NULL, observed_invocation TEXT NOT NULL,
        old_pid_alive INTEGER NOT NULL,
        FOREIGN KEY(command_id) REFERENCES commands(id)
    )""")
    if path.stat().st_mode & 0o077:
        raise AssertionError("journal unexpectedly visible to other OS users")
    return db


def worker(root, unit, operation_id, attempt, executable, when):
    if when not in ("before", "after"):
        raise AssertionError("invalid crash injection point")
    valid_unit(unit)
    valid_app(executable.parent)
    if not re.fullmatch(r"op[a-f0-9]{12}", operation_id):
        raise AssertionError("invalid research command identifier")
    digest = hashlib.sha256(executable.read_bytes()).hexdigest()
    db = connection(root)
    try:
        state = os_state(unit)
        if (state.get("ActiveState") != "active" or
                state.get("InvocationID") != attempt["invocation"] or
                not live_exact(attempt["pid"], attempt["tick"])):
            raise AssertionError("exact OS attempt changed before admission")
        # Research-only root test approval, not authorization by CLI/UI.
        # Commit the intent and immutable exact OS target BEFORE any effect.
        db.execute("BEGIN IMMEDIATE")
        db.execute("""INSERT INTO commands
            (id, unit, invocation, pid, start_tick, executable_sha256, verb, phase)
            VALUES (?, ?, ?, ?, ?, ?, 'stop', 'submitted')""",
            (operation_id, unit, attempt["invocation"],
             attempt["pid"], attempt["tick"], digest))
        db.execute("COMMIT")
        if when == "before":
            os._exit(EXIT_BEFORE)  # durable intent, NO OS call
        # Trust the bound unit only from root-owned test registration.
        # systemctl stop is NOT atomic with the preceding InvocationID check.
        run(["systemctl", "--system", "--no-pager", "--no-ask-password",
             "stop", unit], timeout=16)
        os._exit(EXIT_AFTER)  # real OS effect, NO completed receipt
    finally:
        db.close()


def reconcile(root, operation_id):
    # Critical property: reconciliation is observation-only. It issues
    # neither systemctl start/stop nor a replacement authorization.
    db = connection(root)
    try:
        db.row_factory = sqlite3.Row
        row = db.execute(
            "SELECT * FROM commands WHERE id = ?", (operation_id,)
        ).fetchone()
        if row is None:
            raise AssertionError("no protected recorded command")
        state = os_state(row["unit"])
        current = state.get("InvocationID", "")
        os_phase = state.get("ActiveState", "")
        old_exists = live_exact(row["pid"], row["start_tick"])
        if current and current != row["invocation"] and os_phase == "active":
            verdict = "superseded_new_attempt_no_replay"
        elif (current == row["invocation"] and os_phase == "active"
              and old_exists):
            verdict = "unknown_still_active_no_replay"
        elif os_phase in ("inactive", "failed") and not old_exists:
            verdict = "observed_inactive_old_attempt_no_replay"
        else:
            verdict = "unknown_inconsistent_no_replay"
        db.execute("BEGIN IMMEDIATE")
        db.execute("""INSERT INTO observations
            (command_id, verdict, unit_state, observed_invocation, old_pid_alive)
            VALUES (?, ?, ?, ?, ?)""",
            (operation_id, verdict, os_phase, current, int(old_exists)))
        db.execute("COMMIT")
        return {"verdict": verdict, "unit_state": os_phase,
                "observed_invocation": current, "old_pid_alive": old_exists,
                "journal_phase": row["phase"]}
    finally:
        db.close()


def adversary(root, unit, pid):
    if os.getuid() != UNTRUSTED or os.geteuid() != UNTRUSTED:
        raise AssertionError("attacker must run under real UID 60090")
    with open("/proc/self/status", encoding="utf-8") as f:
        effective = next(line for line in f if line.startswith("CapEff:"))
    if int(effective.split(":", 1)[1], 16):
        raise AssertionError("attacker has effective Linux capabilities")
    # The fixture's sudo-enabled CI account is explicitly NOT this attacker.
    if run(["sudo", "-n", "true"], check=False).returncode == 0:
        raise AssertionError("attacker has ambient sudo")
    try:
        (root / "commands.sqlite").read_bytes()
    except PermissionError:
        pass
    else:
        raise AssertionError("untrusted same-UID client can read protected log")
    try:
        (root / "commands.sqlite").write_bytes(b"forged")
    except PermissionError:
        pass
    else:
        raise AssertionError("untrusted same-UID client can modify protected log")
    if run(["systemctl", "--system", "--no-ask-password", "stop", unit],
           check=False).returncode == 0:
        raise AssertionError("untrusted client can stop system owner")
    try:
        os.kill(pid, signal.SIGTERM)
    except PermissionError:
        pass
    else:
        raise AssertionError("untrusted client signalled real Counter")
    print("RESEARCH_M59_UNKNOWN_OUTCOME_UNTRUSTED_OS_AND_JOURNAL_DENIED",
          flush=True)


def controller(pinned):
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("real systemd Linux nonroot coordinator required")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("real systemd PID1 required")
    if run(["git", "-C", str(pinned), "rev-parse", "HEAD"]).stdout.strip() != PINNED:
        raise AssertionError("must reuse exact pinned real Counter source")
    binary = pinned / "target/debug/counter_resolved_facade_host"
    if not binary.is_file():
        raise AssertionError("pinned real Counter binary not built")
    test_script = Path(__file__).resolve()
    nonce = uuid.uuid4().hex[:12]
    unit = valid_unit(f"{PFX}{nonce}.service")
    directory = valid_app(Path(DIR + nonce))
    installed = directory / "counter_resolved_facade_host"
    root = valid_root(Path(tempfile.mkdtemp(prefix=ROOT)))
    created_install = False
    started = False
    try:
        run(elevated("install", "-d", "-o", "root", "-g", "root",
                     "-m", "0755", str(directory)))
        created_install = True
        run(elevated("install", "-o", "root", "-g", "root", "-m",
                     "0755", str(binary), str(installed)))
        if hashlib.sha256(binary.read_bytes()).hexdigest() != hashlib.sha256(
            installed.read_bytes()
        ).hexdigest():
            raise AssertionError("installed pinned real Counter differs")
        run(elevated("chown", "root:root", str(root)))
        run(elevated("chmod", "0700", str(root)))
        # Root-owned isolated script copy: hostile UID 60090 can execute it,
        # but it cannot mutate the script, root DB or obtain root admission.
        run(elevated("install", "-o", "root", "-g", "root", "-m",
                     "0755", str(test_script), str(directory / "probe.py")))
        root_script = directory / "probe.py"
        # systemd is the process owner; no automatic service restart.
        dynamic_user = "rieselj" + nonce[:9]
        run(elevated("systemd-run", "--system", "--no-ask-password",
                     "--unit", unit, "--property=Type=exec",
                     "--property=Restart=no", "--property=DynamicUser=yes",
                     "--property=User=" + dynamic_user,
                     "--property=KillMode=control-group",
                     "--property=TimeoutStopSec=4s",
                     "--property=WorkingDirectory=/tmp", "/bin/sh", "-c",
                     f"sleep 600 | {installed}"))
        started = True
        initial = observe_running(unit, installed)
        if initial["pid"] == os.getpid():
            raise AssertionError("observed coordinator instead of Riesel app")
        uid_line = next(x for x in
                        Path(f"/proc/{initial['pid']}/status").read_text().splitlines()
                        if x.startswith("Uid:"))
        service_uid = int(uid_line.split()[1])
        if service_uid in (0, UNTRUSTED, os.geteuid()):
            raise AssertionError("actual Counter not owned by separate OS principal")
        print("RESEARCH_M59_UNKNOWN_REAL_RIESEL_OS_OWNER_VERIFIED", flush=True)

        attacker_result = run(elevated(
            "setpriv", "--reuid", str(UNTRUSTED), "--regid", str(UNTRUSTED),
            "--clear-groups", "/usr/bin/python3", str(root_script),
            "--attack", str(root), unit, str(initial["pid"])), check=False)
        if (attacker_result.returncode or
            "RESEARCH_M59_UNKNOWN_OUTCOME_UNTRUSTED_OS_AND_JOURNAL_DENIED"
                not in attacker_result.stdout):
            raise AssertionError(
                f"attacker did not demonstrate denied boundaries: {attacker_result}"
            )
        before_id = "op" + uuid.uuid4().hex[:12]
        cmd = [str(root_script), "--mutate", str(root), unit,
               before_id, json.dumps(initial), str(installed)]
        before = run(elevated("/usr/bin/python3", *cmd, "before"),
                     check=False)
        if before.returncode != EXIT_BEFORE:
            raise AssertionError(f"before-effect worker did not crash at gate: {before}")
        state = os_state(unit)
        if (state.get("ActiveState") != "active" or
                state.get("InvocationID") != initial["invocation"] or
                not live_exact(initial["pid"], initial["tick"])):
            raise AssertionError("real app changed in pre-effect crash control")
        response = run(elevated("/usr/bin/python3", str(root_script),
                                "--reconcile", str(root), before_id))
        before_receipt = json.loads(response.stdout.strip())
        if before_receipt["verdict"] != "unknown_still_active_no_replay":
            raise AssertionError(f"pre-effect crash misclassified: {before_receipt}")
        print("RESEARCH_M59_UNKNOWN_BEFORE_EFFECT_DURABLE_UNCERTAIN_NO_RETRY",
              flush=True)

        # A separate ROOT test operator deliberately restarts the actual unit
        # to create another OS InvocationID. No blind recovery of the old
        # prepared command is allowed to stop this replacement generation.
        run(elevated("systemctl", "--system", "--no-pager", "restart", unit))
        second = observe_running(unit, installed, previous=initial["invocation"])
        if second["invocation"] == initial["invocation"]:
            raise AssertionError("systemd did not issue new launch attempt")
        stale = run(elevated("/usr/bin/python3", str(root_script),
                             "--reconcile", str(root), before_id))
        stale_receipt = json.loads(stale.stdout.strip())
        if stale_receipt["verdict"] != "superseded_new_attempt_no_replay":
            raise AssertionError(f"old command contaminated new attempt: {stale_receipt}")
        now = os_state(unit)
        if (now["ActiveState"] != "active" or
                now["InvocationID"] != second["invocation"]):
            raise AssertionError("recovery damaged new running application")
        print("RESEARCH_M59_UNKNOWN_RECONCILE_FENCES_NEW_OS_INVOCATION",
              flush=True)

        # Actually stop the second REAL Riesel attempt, then crash the
        # trusted mutation worker before it can acknowledge completion.
        after_id = "op" + uuid.uuid4().hex[:12]
        cmd = [str(root_script), "--mutate", str(root), unit,
               after_id, json.dumps(second), str(installed)]
        after = run(elevated("/usr/bin/python3", *cmd, "after"),
                    check=False, timeout=26)
        if after.returncode != EXIT_AFTER:
            raise AssertionError(f"post-effect worker failed: {after}")
        observed = os_state(unit)
        if (observed.get("ActiveState") != "inactive" or
                live_exact(second["pid"], second["tick"])):
            raise AssertionError(
                f"real Counter still running after root stop/crash: {observed}"
            )
        after_out = run(elevated("/usr/bin/python3", str(root_script),
                                 "--reconcile", str(root), after_id))
        after_receipt = json.loads(after_out.stdout.strip())
        if after_receipt["verdict"] != "observed_inactive_old_attempt_no_replay":
            raise AssertionError(f"real stop effect not observed: {after_receipt}")
        if after_receipt["journal_phase"] != "submitted":
            raise AssertionError("crashed worker falsely claimed completed receipt")
        print("RESEARCH_M59_UNKNOWN_AFTER_EFFECT_REAL_RIESEL_STOP_RECONCILED",
              flush=True)

        # Controller restarts must not invoke another stop or lose commands.
        again = run(elevated("/usr/bin/python3", str(root_script),
                             "--reconcile", str(root), after_id))
        if json.loads(again.stdout)["verdict"] != after_receipt["verdict"]:
            raise AssertionError("repeated restart changed observed effect")
        counts = run(elevated("/usr/bin/python3", str(root_script),
                              "--inspect", str(root)))
        count = json.loads(counts.stdout)
        if count != {"commands": 2, "observations": 4}:
            raise AssertionError(f"durable command/observation journal mismatch: {count}")
        print("RESEARCH_M59_UNKNOWN_DURABLE_RECEIPTS_SURVIVE_CONTROLLERS", flush=True)
        print("RESEARCH_M59_UNKNOWN_EFFECT_CRASH_RESEARCH_PASS", flush=True)
    finally:
        if started:
            run(elevated("systemctl", "--system", "--no-pager", "stop", unit),
                check=False)
            run(elevated("systemctl", "--system", "--no-pager",
                         "reset-failed", unit), check=False)
        if created_install:
            run(elevated("rm", "-rf", "--", str(valid_app(directory))), check=False)
        if root.exists():
            run(elevated("rm", "-rf", "--", str(valid_root(root))), check=False)


def inspect(root):
    db = connection(root)
    try:
        commands = db.execute("SELECT count(*) FROM commands").fetchone()[0]
        observations = db.execute("SELECT count(*) FROM observations").fetchone()[0]
        return {"commands": commands, "observations": observations}
    finally:
        db.close()


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--exercise":
        controller(Path(sys.argv[2]).resolve())
    elif len(sys.argv) == 8 and sys.argv[1] == "--mutate":
        worker(Path(sys.argv[2]), sys.argv[3], sys.argv[4],
               json.loads(sys.argv[5]), Path(sys.argv[6]), sys.argv[7])
    elif len(sys.argv) == 4 and sys.argv[1] == "--reconcile":
        print(json.dumps(reconcile(Path(sys.argv[2]), sys.argv[3])))
    elif len(sys.argv) == 3 and sys.argv[1] == "--inspect":
        print(json.dumps(inspect(Path(sys.argv[2]))))
    elif len(sys.argv) == 5 and sys.argv[1] == "--attack":
        adversary(Path(sys.argv[2]), sys.argv[3], int(sys.argv[4]))
    else:
        raise SystemExit("invalid research command invocation")
