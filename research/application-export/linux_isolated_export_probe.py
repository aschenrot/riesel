#!/usr/bin/env python3
"""Actual OS-hosted Riesel Counter with a bounded local capability grant gate.

All privileged operations are controlled by a ROOT test coordinator, not a
real operator-approved Manager or trusted production grant issuer. Numeric
UIDs are disposable *principals*, no NSS accounts or credentials installed.
This research-only fixture mutates only a uniquely named systemd transient
unit and two tightly validated /opt and /run test directories.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import pwd
import re
import socket
import subprocess
import sys
import time
import uuid

PINNED = "5201e61cc74bab778dfc63a3788f1708b2530f21"
ALLOWED = 60090
OUTSIDER = 60091
APP_UID = 60092
UNIT_PREFIX = "riesel-m60-export-"
INSTALL_PREFIX = "/opt/riesel-m60-protected-export-"
RUNTIME_PREFIX = "/run/riesel-m60-protected-export-"
APP = "research.private.counter"
INTERFACE = "ui.counter.v1"


def invoke(argv, *, check=True, timeout=18):
    p = subprocess.run(
        [str(x) for x in argv], text=True, input="",
        capture_output=True, cwd="/tmp", timeout=timeout, check=False,
    )
    if check and p.returncode:
        raise AssertionError(
            f"research command failed rc={p.returncode} "
            f"argv={argv[:5]!r} stderr={p.stderr[-850:]!r} "
            f"stdout={p.stdout[-350:]!r}"
        )
    return p


def sudo(*args):
    return ["sudo", "-n", *args]


def known_unit(unit):
    if not re.fullmatch(r"riesel-m60-export-[a-f0-9]{12}\.service", unit):
        raise AssertionError("refusing nonresearch OS service name")
    return unit


def known_path(path, kind):
    pat = (r"/opt/riesel-m60-protected-export-[a-f0-9]{12}"
           if kind == "install"
           else r"/run/riesel-m60-protected-export-[a-f0-9]{12}")
    if not re.fullmatch(pat, str(path)):
        raise AssertionError(f"refusing nonresearch {kind} root")
    return path


def status(unit):
    known_unit(unit)
    p = invoke(["systemctl", "--system", "--no-pager", "show", unit,
                "--property=MainPID", "--property=ActiveState",
                "--property=InvocationID", "--property=PrivateNetwork"])
    return dict(x.split("=", 1) for x in p.stdout.splitlines() if "=" in x)


def ns_inode(pid):
    return os.stat(f"/proc/{pid}/ns/net").st_ino


def uid_of(pid):
    s = Path(f"/proc/{pid}/status").read_text()
    return int(next(x for x in s.splitlines() if x.startswith("Uid:")).split()[1])


def run_client(script, sock, op, amount, epoch, real_uid, claimed_uid=None):
    cmd = sudo(
        "setpriv", "--reuid", str(real_uid), "--regid", str(real_uid),
        "--clear-groups",
        "/usr/bin/python3", str(script), "--client", str(sock), op,
        str(amount), epoch, str(real_uid if claimed_uid is None else claimed_uid),
    )
    p = invoke(cmd)
    return json.loads(p.stdout.strip())


def run_admin(script, sock, payload):
    p = invoke(sudo("/usr/bin/python3", str(script), "--admin",
                    str(sock), json.dumps(payload)))
    return json.loads(p.stdout.strip())


def expect_error(result, error):
    if result.get("error") != error:
        raise AssertionError(f"expected {error!r}, received {result!r}")


def wait_ready(runtime, unit, seconds=27):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        state = status(unit)
        p = runtime / "status.json"
        if p.is_file():
            result = json.loads(p.read_text())
            if (result.get("gateway_ready")
                and state.get("InvocationID")
                and state.get("ActiveState") == "active"):
                return result, state
        last = state
        time.sleep(0.2)
    log = invoke(sudo("journalctl", "--no-pager", "--unit", unit,
                      "--lines", "30"), check=False)
    raise AssertionError(
        f"protected gateway did not become callable: state={last}; "
        f"journal={log.stdout[-1400:]!r}"
    )


def main(pinned):
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("needs an ordinary-user Linux coordinator")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("needs real systemd PID1")
    if ns_inode(1) != ns_inode(os.getpid()):
        raise AssertionError("coordinator unexpectedly outside host network ns")
    for cmd in ("systemd-run", "setpriv", "nsenter", "sudo"):
        if invoke(["/usr/bin/which", cmd], check=False).returncode:
            raise AssertionError(f"missing real OS fixture tool: {cmd}")
    if invoke(sudo("true"), check=False).returncode:
        raise AssertionError("no trusted root fixture provisioner")
    for real_uid in (ALLOWED, OUTSIDER, APP_UID):
        try:
            row = pwd.getpwuid(real_uid)
        except KeyError:
            continue
        raise AssertionError(
            f"refuse to use existing OS account {real_uid}: {row.pw_name}"
        )
    if invoke(["git", "-C", str(pinned), "rev-parse", "HEAD"]).stdout.strip() != PINNED:
        raise AssertionError("must use unchanged pinned real SDK Counter")
    built = pinned / "target/debug/counter_resolved_facade_host"
    if not built.is_file():
        raise AssertionError("real Riesel Counter not built")
    nonce = uuid.uuid4().hex[:12]
    unit = known_unit(UNIT_PREFIX + nonce + ".service")
    install = known_path(Path(INSTALL_PREFIX + nonce), "install")
    runtime = known_path(Path(RUNTIME_PREFIX + nonce), "runtime")
    binary = install / "counter_resolved_facade_host"
    script = install / "linux_isolated_export_server.py"
    source = Path(__file__).parent / "linux_isolated_export_server.py"
    ready = False
    installed = False
    runtime_created = False
    try:
        invoke(sudo("install", "-d", "-m", "0755", "-o", "root", "-g",
                    "root", str(install)))
        installed = True
        invoke(sudo("install", "-d", "-m", "0755", "-o", "root", "-g",
                    "root", str(runtime)))
        runtime_created = True
        invoke(sudo("install", "-m", "0755", "-o", "root", "-g", "root",
                    str(built), str(binary)))
        invoke(sudo("install", "-m", "0755", "-o", "root", "-g", "root",
                    str(source), str(script)))
        # systemd owns the service even when launching coordinator exits.
        # PrivateNetwork is REQUIRED, no fallback to host namespace.
        ready = True
        invoke(sudo(
            "systemd-run", "--system", "--no-block", "--no-ask-password",
            "--unit", unit, "--property=Type=exec",
            "--property=PrivateNetwork=yes", "--property=Restart=no",
            "--property=KillMode=control-group",
            "--property=TimeoutStopSec=7s",
            "--property=WorkingDirectory=/tmp",
            "/usr/bin/python3", str(script), "--serve",
            str(install), str(runtime)
        ))
        evidence, unit_state = wait_ready(runtime, unit)
        app_pid = evidence["app_pid"]
        if (evidence.get("app_uid") != APP_UID or uid_of(app_pid) != APP_UID
            or not unit_state.get("InvocationID")):
            raise AssertionError("actual Counter OS principal/attempt wrong")
        main_pid = int(unit_state.get("MainPID", "0"))
        if (main_pid < 2 or uid_of(main_pid) != 0 or
            ns_inode(main_pid) == ns_inode(os.getpid()) or
            ns_inode(main_pid) != ns_inode(app_pid)):
            raise AssertionError("service network namespace not really private")
        print("RESEARCH_M60_REAL_COUNTER_PROTECTED_IN_PRIVATE_NETWORK_NS",
              flush=True)
        # A known copy of the private backend address is NOT a credential.
        # Host namespace has its own 127.0.0.1; it must not directly reach
        # the Counter's original unprotected TCP socket.
        _, port = evidence["backend"].rsplit(":", 1)
        try:
            conn = socket.create_connection(("127.0.0.1", int(port)), timeout=0.5)
        except (OSError, TimeoutError):
            pass
        else:
            conn.close()
            raise AssertionError(
                "host network can reach unauthenticated Counter listener")
        enter = invoke(sudo(
            "setpriv", "--reuid", str(ALLOWED),
            "--regid", str(ALLOWED), "--clear-groups",
            "nsenter", "--net", f"/proc/{app_pid}/ns/net", "/usr/bin/true"
        ), check=False)
        if enter.returncode == 0:
            raise AssertionError("unprivileged UI UID could enter app network ns")
        print("RESEARCH_M60_DIRECT_TCP_BYPASS_AND_UNPRIVILEGED_NETNS_ENTRY_DENIED",
              flush=True)

        public = runtime / "public.sock"
        control = runtime / "owner/control.sock"
        epoch = evidence["epoch"]
        if run_admin(script, control, {"action": "status"})["epoch"] != epoch:
            raise AssertionError("gateway status/attempt identity mismatch")
        # Hostile login/client account can reach public endpoint but cannot
        # access privileged policy issuer or impersonate its root UID.
        probe_admin = invoke(sudo(
            "setpriv", "--reuid", str(OUTSIDER),
            "--regid", str(OUTSIDER), "--clear-groups",
            "/usr/bin/python3", str(script), "--admin",
            str(control), json.dumps({"action": "grant", "uid": OUTSIDER,
                                      "operations": ["add"]})
        ), check=False)
        if probe_admin.returncode == 0:
            raise AssertionError("untrusted UID can directly grant itself rights")
        denied = run_client(script, public, "value", 0, epoch, ALLOWED)
        expect_error(denied, "not_authorized")
        print("RESEARCH_M60_DEFAULT_DENY_KERNEL_CLIENT_UID_NOT_CLAIMED_ROLE",
              flush=True)

        if run_admin(script, control, {"action": "grant", "uid": ALLOWED,
                                      "operations": ["value"]}) != {
                                          "ok": True, "operations": ["value"]}:
            raise AssertionError("trusted research read-only grant not installed")
        read = run_client(script, public, "value", 0, epoch, ALLOWED)
        if read.get("result") != {"Ok": 0} or read["observed_uid"] != ALLOWED:
            raise AssertionError(f"actual Counter read not authorized: {read}")
        expect_error(run_client(script, public, "add", 1, epoch, ALLOWED),
                     "not_authorized")
        outsider = run_client(
            script, public, "add", 1, epoch, OUTSIDER,
            claimed_uid=ALLOWED
        )
        expect_error(outsider, "not_authorized")
        if outsider["observed_uid"] != OUTSIDER:
            raise AssertionError("untrusted claimed principal bypassed SO_PEERCRED")
        expect_error(run_client(script, public, "value", 0, "0" * 32, ALLOWED),
                     "stale_gateway_epoch")
        print("RESEARCH_M60_KERNEL_ATTESTED_READ_WRITE_AND_STALE_GATE",
              flush=True)

        run_admin(script, control, {"action": "grant", "uid": ALLOWED,
                                    "operations": ["value", "add"]})
        first = run_client(script, public, "add", 2, epoch, ALLOWED)
        if first.get("result") != {"Ok": 2}:
            raise AssertionError(f"real Riesel Counter add failed: {first}")
        # Intentional SAME-UID negative control. These are two different
        # processes from the same untrusted account, both accepted by a
        # UID-level grant. This profile is NOT protected app identity.
        second = run_client(script, public, "add", 3, epoch, ALLOWED)
        if second.get("result") != {"Ok": 5} or (
            first.get("observed_pid") == second.get("observed_pid")
        ):
            raise AssertionError(
                "two separate same-UID clients not demonstrated")
        print("RESEARCH_M60_SAME_UID_UNREGISTERED_CLIENT_INHERITS_UID_GRANT",
              flush=True)
        if run_client(script, public, "value", 0, epoch, ALLOWED).get(
            "result"
        ) != {"Ok": 5}:
            raise AssertionError("real Counter was not mutated through gate")

        # Deterministically pause a third command BEFORE authorization and
        # dispatch, revoke the UID, and check that the command never reaches
        # the real Riesel capability despite the client already connecting.
        run_admin(script, control, {"action": "arm_pause"})
        child_cmd = sudo(
            "setpriv", "--reuid", str(ALLOWED), "--regid", str(ALLOWED),
            "--clear-groups", "/usr/bin/python3", str(script), "--client",
            str(public), "add", "6", epoch, str(ALLOWED)
        )
        child = subprocess.Popen(child_cmd, cwd="/tmp", text=True,
                                 stdout=subprocess.PIPE,
                                 stderr=subprocess.PIPE)
        try:
            for _ in range(60):
                if run_admin(script, control, {"action": "status"}).get("paused"):
                    break
                time.sleep(0.1)
            else:
                raise AssertionError("pre-dispatch research pause not reached")
            run_admin(script, control, {"action": "revoke", "uid": ALLOWED})
            run_admin(script, control, {"action": "resume"})
            stdout, stderr = child.communicate(timeout=13)
            if child.returncode != 0:
                raise AssertionError(
                    f"paused client failed: {child.returncode}: {stderr[-450:]}")
            expect_error(json.loads(stdout), "not_authorized")
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=3)
        expect_error(run_client(script, public, "add", 1, epoch, ALLOWED),
                     "not_authorized")
        run_admin(script, control, {"action": "grant", "uid": ALLOWED,
                                    "operations": ["value"]})
        final = run_client(script, public, "value", 0, epoch, ALLOWED)
        if final.get("result") != {"Ok": 5}:
            raise AssertionError(
                "revoked in-flight mutation reached Riesel backend")
        print("RESEARCH_M60_PRE_DISPATCH_REVOKE_BLOCKS_REAL_COUNTER_MUTATION",
              flush=True)
        print("RESEARCH_M60_LOCAL_CAPABILITY_OS_ISOLATION_PROBE_PASS",
              flush=True)
    finally:
        if ready:
            invoke(sudo("systemctl", "--system", "--no-pager", "stop", unit),
                   check=False)
            invoke(sudo("systemctl", "--system", "--no-pager", "reset-failed", unit),
                   check=False)
        if runtime_created:
            invoke(sudo("rm", "-rf", "--", str(known_path(runtime, "runtime"))),
                   check=False)
        if installed:
            invoke(sudo("rm", "-rf", "--", str(known_path(install, "install"))),
                   check=False)


if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] != "--exercise":
        raise SystemExit("usage: probe.py --exercise </pinned/PR90/root>")
    main(Path(sys.argv[2]).resolve())
