#!/usr/bin/env python3
"""Research-only M-S2: protected exact-action admission against two same-UID clients.

Runs with an unmodified, pinned, real Riesel Counter application under a Linux
systemd DynamicUser service. A root broker receives proposals from independent
UID-60090 clients but accepts authorization ONLY over a root-only control socket.
An independently privileged test controller explicitly approves one proposal.

This proves only a *protected test authority/policy-bound admission* boundary.
The privileged test controller is NOT a human authentication or trusted GUI.
The CI runner has ambient passwordless sudo; therefore its own UID is NOT an
eligible hostile principal. No new installed Manager or public Riesel API.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import secrets
import select
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import uuid

ATTACKER_UID = 60090
PINNED_SOURCE = "5201e61cc74bab778dfc63a3788f1708b2530f21"
UNIT_PREFIX = "riesel-m59-exact-"
OPT_PREFIX = "/opt/riesel-m59-exact-"
ROOT_PREFIX = "riesel-m59-exact-control-"


def run(argv, *, timeout=12, check=True):
    p = subprocess.run(
        [str(a) for a in argv], cwd="/tmp", stdin=subprocess.DEVNULL,
        capture_output=True, text=True, timeout=timeout, check=False,
    )
    if check and p.returncode:
        raise AssertionError(
            f"{[str(a) for a in argv[:5]]!r} exit={p.returncode} "
            f"out={p.stdout[-400:]!r} err={p.stderr[-900:]!r}"
        )
    return p


def sudo(*args):
    return ["sudo", "-n", *args]


def restricted_unit(unit):
    if not re.fullmatch(r"riesel-m59-exact-[a-f0-9]{12}\.service", unit):
        raise AssertionError("refusing nonresearch system unit")
    return unit


def restricted_app_dir(path):
    if not re.fullmatch(r"/opt/riesel-m59-exact-[a-f0-9]{12}", str(path)):
        raise AssertionError("refusing nonresearch installation path")
    return path


def system_show(unit, *, elevated=False):
    restricted_unit(unit)
    cmd = ["systemctl", "--system", "--no-pager", "--no-ask-password",
           "show", unit, "--property=MainPID", "--property=InvocationID",
           "--property=ActiveState", "--property=User"]
    result = run(sudo(*cmd) if elevated else cmd)
    return dict(line.split("=", 1) for line in result.stdout.splitlines()
                if "=" in line)


def process_uid(pid):
    status = Path(f"/proc/{pid}/status").read_text()
    return int(next(x for x in status.splitlines()
                    if x.startswith("Uid:")).split()[1])


def actual_counter_pid(exe):
    hits = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            first = (proc / "cmdline").read_bytes().split(b"\0", 1)[0]
        except (OSError, ProcessLookupError):
            continue
        if first == os.fsencode(exe):
            hits.append(int(proc.name))
    if len(hits) != 1:
        raise AssertionError(f"expected one actual Riesel Counter: {hits}")
    return hits[0]


def transact(path, obj, *, timeout=6):
    payload = (json.dumps(obj, separators=(",", ":")) + "\n").encode()
    if len(payload) > 4096:
        raise AssertionError("research request too long")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(timeout)
        s.connect(str(path))
        s.sendall(payload)
        response = bytearray()
        while b"\n" not in response and len(response) < 8192:
            packet = s.recv(8192 - len(response))
            if not packet:
                break
            response.extend(packet)
    if not response.endswith(b"\n"):
        raise AssertionError(f"invalid broker reply: {bytes(response[-250:])!r}")
    return json.loads(response)


def receive_one(conn):
    conn.settimeout(3)
    buf = bytearray()
    while b"\n" not in buf and len(buf) < 4096:
        data = conn.recv(4096 - len(buf))
        if not data:
            break
        buf.extend(data)
    if b"\n" not in buf or len(buf) >= 4096:
        return {"kind": "invalid"}
    try:
        data = json.loads(buf.split(b"\n", 1)[0])
    except (ValueError, UnicodeDecodeError):
        return {"kind": "invalid"}
    return data if isinstance(data, dict) else {"kind": "invalid"}


def send_one(conn, value):
    conn.sendall((json.dumps(value, sort_keys=True) + "\n").encode())


def peer_uid(conn):
    raw = conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
    _pid, uid, _gid = struct.unpack("3i", raw)
    return uid


def broker(root, unit, invocation):
    """Actual OS-protected mock policy authority; never a public Manager API."""
    restricted_unit(unit)
    if os.geteuid() != 0 or root.is_symlink() or root.stat().st_uid != 0:
        raise AssertionError("broker requires protected root-owned directory")
    private = root / "private"
    if private.is_symlink() or private.stat().st_uid != 0:
        raise AssertionError("private socket parent must be protected")
    if private.stat().st_mode & 0o077:
        raise AssertionError("private socket directory accessible to ordinary user")
    public_sock, admin_sock = root / "public.sock", private / "admin.sock"
    pending, receipts = {}, {}
    with socket.socket(socket.AF_UNIX) as pub, socket.socket(socket.AF_UNIX) as adm:
        pub.bind(str(public_sock))
        os.chmod(public_sock, 0o666)
        pub.listen(8)
        adm.bind(str(admin_sock))
        os.chmod(admin_sock, 0o600)
        adm.listen(4)
        print("RESEARCH_M59_ROOT_BROKER_READY", flush=True)
        deadline = time.monotonic() + 100
        while time.monotonic() < deadline:
            readable, _, _ = select.select([pub, adm], [], [], 0.5)
            for channel in readable:
                conn, _ = channel.accept()
                with conn:
                    try:
                        uid = peer_uid(conn)
                        msg = receive_one(conn)
                        kind = msg.get("kind")
                        if channel is pub:
                            # This endpoint trusts neither process identity nor an
                            # unprotected request_id as authorization.
                            if kind == "propose" and uid == ATTACKER_UID:
                                if (msg.get("unit"), msg.get("invocation"), msg.get("verb")) != (
                                    unit, invocation, "stop"):
                                    answer = {"status": "rejected", "reason": "target"}
                                else:
                                    proposal = secrets.token_hex(16)
                                    pending[proposal] = {
                                        "unit": unit, "invocation": invocation,
                                        "verb": "stop", "uid": uid,
                                    }
                                    answer = {"status": "pending", "request_id": proposal}
                            elif kind == "status" and uid == ATTACKER_UID:
                                req = msg.get("request_id")
                                answer = receipts.get(req, (
                                    {"status": "pending"} if req in pending else
                                    {"status": "unknown_request"}))
                            else:
                                answer = {"status": "rejected", "reason": "no_public_authority"}
                        elif uid != 0:
                            answer = {"status": "rejected", "reason": "protected_channel"}
                        elif kind == "approve":
                            req = msg.get("request_id")
                            expected = {"unit": unit, "invocation": invocation, "verb": "stop",
                                        "uid": ATTACKER_UID}
                            if req not in pending:
                                answer = {"status": "rejected", "reason": "already_used_or_unknown"}
                            elif pending[req] != expected or any(
                                msg.get(k) != expected[k] for k in ("unit", "invocation", "verb")
                            ):
                                answer = {"status": "rejected", "reason": "unmatched_exact_action"}
                            else:
                                # Consume prior to dispatch; there is no generic retry.
                                del pending[req]
                                receipts[req] = {"status": "unknown_after_admission"}
                                current = system_show(unit)
                                if (current.get("InvocationID") != invocation or
                                    current.get("ActiveState") != "active"):
                                    answer = {"status": "rejected", "reason": "stale_os_attempt"}
                                    receipts[req] = answer
                                else:
                                    result = run(
                                        ["systemctl", "--system", "--no-pager",
                                         "--no-ask-password", "stop", unit],
                                        timeout=12, check=False,
                                    )
                                    current = system_show(unit)
                                    if result.returncode == 0 and current.get("ActiveState") == "inactive":
                                        answer = {"status": "completed", "request_id": req}
                                    else:
                                        answer = {"status": "unknown_after_admission",
                                                  "request_id": req}
                                    receipts[req] = answer
                        elif kind == "shutdown":
                            answer = {"status": "bye"}
                            send_one(conn, answer)
                            return
                        else:
                            answer = {"status": "rejected", "reason": "protected_channel"}
                        send_one(conn, answer)
                    except (OSError, ValueError, AssertionError) as exc:
                        # A failed OS command after admission must not be
                        # misreported as definitely without effects.
                        send_one(conn, {"status": "unknown_or_invalid", "type": type(exc).__name__})
        raise AssertionError("research broker deadline exceeded")


def as_untrusted(script, root, msg):
    cmd = ["setpriv", "--reuid", str(ATTACKER_UID), "--regid", str(ATTACKER_UID),
           "--clear-groups", "/usr/bin/python3", str(script),
           "--client", str(root), json.dumps(msg)]
    result = run(cmd, timeout=12)
    return json.loads(result.stdout)


def as_admin(script, root, msg):
    result = run(sudo("/usr/bin/python3", str(script),
                      "--admin", str(root), json.dumps(msg)))
    return json.loads(result.stdout)


def main(pinned):
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("requires Linux nonroot test runner")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("requires actual systemd")
    if run(["git", "-C", str(pinned), "rev-parse", "HEAD"]).stdout.strip() != PINNED_SOURCE:
        raise AssertionError("must build unchanged pinned PR #90 source")
    script = Path(__file__).resolve()
    binary = pinned / "target/debug/counter_resolved_facade_host"
    if not binary.is_file():
        raise AssertionError("real Counter binary not built")
    nonce = uuid.uuid4().hex[:12]
    unit = UNIT_PREFIX + nonce + ".service"
    target = restricted_app_dir(Path(OPT_PREFIX + nonce))
    root = Path(tempfile.mkdtemp(prefix=ROOT_PREFIX))
    broker_proc = None
    installed = target / "counter_resolved_facade_host"
    created_target, started_unit = False, False
    try:
        run(sudo("install", "-d", "-o", "root", "-g", "root", "-m", "0755", str(target)))
        created_target = True
        run(sudo("install", "-o", "root", "-g", "root", "-m", "0755",
                 str(binary), str(installed)))
        run(sudo("chown", "root:root", str(root)))
        run(sudo("chmod", "0755", str(root)))
        run(sudo("install", "-d", "-o", "root", "-g", "root", "-m", "0700",
                 str(root / "private")))

        dynuser = "rieselq" + nonce[:9]
        # Identical unchanged #90 host stdin fixture as research PR #134.
        shell = f"sleep 600 | {installed}"
        run(sudo("systemd-run", "--system", "--no-ask-password",
                 "--unit", unit, "--property=Type=exec",
                 "--property=Restart=no", "--property=DynamicUser=yes",
                 "--property=User=" + dynuser,
                 "--property=KillMode=control-group",
                 "--property=TimeoutStopSec=4s",
                 "--property=WorkingDirectory=/tmp",
                 "/bin/sh", "-c", shell), timeout=20)
        started_unit = True
        original = None
        for _ in range(90):
            observation = system_show(unit, elevated=True)
            if (observation.get("ActiveState") == "active" and
                observation.get("InvocationID") and
                int(observation.get("MainPID", "0")) > 1):
                try:
                    actual = actual_counter_pid(installed)
                except AssertionError:
                    time.sleep(0.2)
                    continue
                original = observation
                break
            time.sleep(0.2)
        if original is None:
            raise AssertionError(f"actual Riesel Counter not active: {system_show(unit, elevated=True)}")
        if process_uid(actual) in (0, ATTACKER_UID, os.geteuid()):
            raise AssertionError("Riesel app did not run under a separate OS principal")
        print("RESEARCH_M59_REAL_RIESEL_DIFFERENT_OS_PRINCIPAL", flush=True)

        # The mock broker is an independently protected root-side process.
        with tempfile.TemporaryFile(mode="w+") as log:
            broker_proc = subprocess.Popen(
                sudo("/usr/bin/python3", str(script), "--broker",
                     str(root), unit, original["InvocationID"]),
                cwd="/tmp", stdin=subprocess.DEVNULL, stdout=log, stderr=log,
            )
            for _ in range(80):
                if (root / "public.sock").exists() and (root / "private/admin.sock").exists():
                    break
                if broker_proc.poll() is not None:
                    log.seek(0)
                    raise AssertionError(f"broker exited {broker_proc.returncode}: {log.read()}")
                time.sleep(0.1)
            else:
                raise AssertionError("protected broker did not establish both channels")
            if (root / "private").stat().st_mode & 0o077:
                raise AssertionError("private authority channel not protected")

            operation = {"kind": "propose", "unit": unit,
                         "invocation": original["InvocationID"], "verb": "stop"}
            legitimate = as_untrusted(script, root, operation)
            hostile = as_untrusted(script, root, operation)
            if (legitimate.get("status") != "pending" or
                hostile.get("status") != "pending" or
                legitimate["request_id"] == hostile["request_id"]):
                raise AssertionError(f"proposals not separately admitted: {legitimate}, {hostile}")
            print("RESEARCH_M59_TWO_INDEPENDENT_SAME_UID_PROPOSALS", flush=True)

            no_auth = as_untrusted(script, root, {
                "kind": "approve", "request_id": hostile["request_id"],
                "unit": unit, "invocation": original["InvocationID"], "verb": "stop",
            })
            if no_auth.get("status") != "rejected":
                raise AssertionError("untrusted public channel elevated a request")
            client_deny = run(["setpriv", "--reuid", str(ATTACKER_UID),
                               "--regid", str(ATTACKER_UID), "--clear-groups",
                               "/usr/bin/python3", str(script), "--admin",
                               str(root), json.dumps({
                                   "kind": "approve", "request_id": hostile["request_id"],
                                   "unit": unit, "invocation": original["InvocationID"],
                                   "verb": "stop",
                               })], check=False)
            if client_deny.returncode == 0:
                raise AssertionError("same-UID client accessed protected approval channel")
            print("RESEARCH_M59_UNTRUSTED_SAME_UID_CANNOT_APPROVE", flush=True)

            # Must test actual shared OS privilege, not assume numeric UID safe.
            no_sudo = run(["setpriv", "--reuid", str(ATTACKER_UID),
                           "--regid", str(ATTACKER_UID), "--clear-groups",
                           "sudo", "-n", "true"], check=False)
            if no_sudo.returncode == 0:
                raise AssertionError("attacker unexpectedly inherited ambient sudo")
            denied_stop = run(["setpriv", "--reuid", str(ATTACKER_UID),
                               "--regid", str(ATTACKER_UID), "--clear-groups",
                               "systemctl", "--system", "--no-ask-password",
                               "stop", unit], check=False)
            if denied_stop.returncode == 0:
                raise AssertionError("attacker stopped OS-owned app directly")
            signal_attempt = run([
                "setpriv", "--reuid", str(ATTACKER_UID),
                "--regid", str(ATTACKER_UID), "--clear-groups",
                "/usr/bin/python3", str(script), "--attack-signal", str(actual),
            ], check=False)
            if (signal_attempt.returncode != 0 or
                "RESEARCH_M59_UNTRUSTED_DIRECT_SIGNAL_DENIED" not in signal_attempt.stdout):
                raise AssertionError(
                    f"direct SIGTERM attack was not correctly denied: "
                    f"{signal_attempt.returncode} {signal_attempt.stderr!r}"
                )
            try:
                os.kill(actual, 0)
            except ProcessLookupError as exc:
                raise AssertionError("app died before approval") from exc
            if system_show(unit, elevated=True)["InvocationID"] != original["InvocationID"]:
                raise AssertionError("untrusted attempt changed app generation")
            print("RESEARCH_M59_DIRECT_OS_BYPASS_DENIED_FOR_TEST_PRINCIPAL", flush=True)

            wrong = as_admin(script, root, {"kind": "approve",
                "request_id": legitimate["request_id"], "unit": unit,
                "invocation": "wrong-generation", "verb": "stop"})
            if wrong.get("reason") != "unmatched_exact_action":
                raise AssertionError(f"mismatched trusted approval not rejected: {wrong}")
            if system_show(unit, elevated=True)["ActiveState"] != "active":
                raise AssertionError("wrong approval stopped app")
            print("RESEARCH_M59_WRONG_ATTEMPT_PRIVILEGED_APPROVAL_DENIED", flush=True)

            # Test controller *stands in for* a future OS-trusted human approval;
            # it uses a protected root-only test socket. CI does NOT prove
            # where human intent comes from or whether GUI input can be spoofed.
            approved = as_admin(script, root, {"kind": "approve",
                "request_id": legitimate["request_id"], "unit": unit,
                "invocation": original["InvocationID"], "verb": "stop"})
            if approved.get("status") != "completed":
                raise AssertionError(f"trusted exact approval failed: {approved}")
            if system_show(unit, elevated=True)["ActiveState"] != "inactive":
                raise AssertionError("app OS owner still active after approved stop")
            for _ in range(30):
                if not Path(f"/proc/{actual}").exists():
                    break
                time.sleep(0.1)
            else:
                raise AssertionError("actual Riesel Counter process did not exit")
            print("RESEARCH_M59_ROOT_APPROVED_EXACT_OPERATION_STOPPED_REAL_RIESEL_APP",
                  flush=True)

            replay = as_admin(script, root, {"kind": "approve",
                "request_id": legitimate["request_id"], "unit": unit,
                "invocation": original["InvocationID"], "verb": "stop"})
            if replay.get("reason") != "already_used_or_unknown":
                raise AssertionError(f"privileged approval replay admitted: {replay}")
            hostile_state = as_untrusted(script, root, {
                "kind": "status", "request_id": hostile["request_id"]})
            if hostile_state.get("status") != "pending":
                raise AssertionError("independent hostile request inherited approval")
            print("RESEARCH_M59_REPLAY_AND_SECOND_SAME_UID_PROPOSAL_DENIED", flush=True)
            print("RESEARCH_M59_PROTECTED_EXACT_ACTION_TEST_AUTHORITY_PASS", flush=True)
    finally:
        if broker_proc is not None:
            try:
                as_admin(script, root, {"kind": "shutdown"})
            except (OSError, AssertionError, ValueError):
                pass
            try:
                broker_proc.wait(timeout=4)
            except subprocess.TimeoutExpired:
                broker_proc.terminate()
                try:
                    broker_proc.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    broker_proc.kill()
                    broker_proc.wait(timeout=4)
        if started_unit:
            run(sudo("systemctl", "--system", "--no-pager", "stop", unit),
                check=False)
            run(sudo("systemctl", "--system", "--no-pager", "reset-failed", unit),
                check=False)
        if created_target:
            run(sudo("rm", "-rf", "--", str(restricted_app_dir(target))), check=False)
        # Root-owned private sockets and root-owned directories are deleted
        # only under the fixture's unique, generated prefix.
        if root.name.startswith(ROOT_PREFIX) and re.fullmatch(
                r"riesel-m59-exact-control-[a-zA-Z0-9_-]+", root.name):
            run(sudo("rm", "-rf", "--", str(root)), check=False)


if __name__ == "__main__":
    if len(sys.argv) == 5 and sys.argv[1] == "--broker":
        broker(Path(sys.argv[2]), sys.argv[3], sys.argv[4])
    elif len(sys.argv) == 4 and sys.argv[1] in ("--client", "--admin"):
        root = Path(sys.argv[2])
        # Distinguish an actually protected socket from a public one.
        if sys.argv[1] == "--admin":
            reply = transact(root / "private/admin.sock", json.loads(sys.argv[3]))
        else:
            reply = transact(root / "public.sock", json.loads(sys.argv[3]))
        print(json.dumps(reply), flush=True)
    elif len(sys.argv) == 3 and sys.argv[1] == "--attack-signal":
        if os.geteuid() != ATTACKER_UID:
            raise AssertionError("signal probe did not execute as untrusted principal")
        try:
            os.kill(int(sys.argv[2]), signal.SIGTERM)
        except PermissionError:
            print("RESEARCH_M59_UNTRUSTED_DIRECT_SIGNAL_DENIED", flush=True)
        else:
            raise AssertionError("untrusted same-UID caller directly stopped app")
    elif len(sys.argv) == 3 and sys.argv[1] == "--exercise":
        main(Path(sys.argv[2]).resolve())
    else:
        raise SystemExit("usage: probe.py --exercise <pinned-tree> | --broker ROOT UNIT EPOCH | --client/--admin ROOT JSON")
