#!/usr/bin/env python3
"""#59 isolated Linux protected system-service owner differential.

Research only: creates one uniquely named transient *system* service that runs
a stock OS sleep executable under a transient systemd DynamicUser UID, and one root-owned
temporary registry-like file. No production Manager, accounts, credentials,
persistent units, or application data are installed or modified.
"""
from __future__ import annotations

import errno
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid

CALLER_UID = 60090
UNIT_PREFIX = "riesel-research-59-secure-"
NAME = re.compile(r"riesel-research-59-secure-[a-f0-9]{12}\.service\Z")
PYTHON = "/usr/bin/python3"


def checked(argv: list[str], *, timeout: int = 10) -> subprocess.CompletedProcess[str]:
    result = subprocess.run(argv, capture_output=True, text=True,
                            timeout=timeout, stdin=subprocess.DEVNULL,
                            check=False, cwd="/tmp")
    if result.returncode:
        raise AssertionError(
            f"command failed (code {result.returncode}): {argv[:4]!r} "
            f"stdout={result.stdout[:500]!r} stderr={result.stderr[:500]!r}"
        )
    return result


def sudo(*args: str) -> list[str]:
    return ["sudo", "-n", *args]


def systemctl(*args: str) -> subprocess.CompletedProcess[str]:
    return checked(sudo("systemctl", "--system", "--no-pager", *args))


def show(unit: str) -> dict[str, str]:
    if not NAME.fullmatch(unit):
        raise AssertionError("refuse non-research unit")
    result = systemctl("show", unit, "--property=Id", "--property=MainPID",
                       "--property=InvocationID", "--property=ActiveState",
                       "--property=SubState", "--property=User")
    values = {}
    for line in result.stdout.splitlines():
        key, sep, value = line.partition("=")
        if sep:
            values[key] = value
    return values


def read_uid(pid: int) -> int:
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith("Uid:"):
            return int(line.split()[1])
    raise AssertionError("no kernel-reported process owner")


def launch_and_crash(unit: str) -> None:
    if not NAME.fullmatch(unit):
        raise AssertionError("refuse non-research unit")
    # Unlike a made-up numeric UID (which systemd 255 rejected with
    # status=217/USER), DynamicUser=yes requests an ephemeral OS-owned
    # principal without editing /etc/passwd or installing permanent users.
    ephemeral_user = "rieselpr" + unit[len(UNIT_PREFIX):len(UNIT_PREFIX) + 10]
    try:
        checked(sudo("systemd-run", "--system", "--no-ask-password",
                     "--unit", unit, "--property=Type=exec",
                     "--property=Restart=no", "--property=KillMode=control-group",
                     "--property=DynamicUser=yes",
                     "--property=User=" + ephemeral_user,
                     "--property=TimeoutStopSec=3s",
                     "/usr/bin/sleep", "100"))
    except AssertionError as error:
        # A transient service creation failure is NOT evidence about OS
        # isolation. Inspect only the uniquely named disposable unit, then
        # let the parent fixture clean up that unit in its finally block.
        state = checked(sudo("systemctl", "--system", "--no-pager", "show",
                             unit, "--property=Result", "--property=ExecMainStatus",
                             "--property=ExecMainCode", "--property=User"))
        logs = subprocess.run(
            sudo("journalctl", "--system", "--no-pager", "-n", "12", "-u", unit),
            capture_output=True, text=True, timeout=7, check=False,
        )
        raise AssertionError(
            f"transient fixture launch failed: {error}; "
            f"unit={state.stdout!r}; journal={logs.stdout[-2200:]!r}"
        ) from error
    # The submitting Manager-like controller vanishes; systemd owns the app.
    os._exit(23)


def attack(unit: str, pid: int, protected_path: str) -> None:
    if os.geteuid() != CALLER_UID:
        raise AssertionError("attacker is not the disposable caller principal")
    with open("/proc/self/status", encoding="utf-8") as info:
        caps = next(line for line in info if line.startswith("CapEff:"))
    if int(caps.split(":", 1)[1], 16) != 0:
        raise AssertionError("attacker retained effective capabilities")
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError as error:
        if error.errno != errno.EPERM:
            raise
        print("DENIED_SIGNAL", flush=True)
    else:
        raise AssertionError("untrusted caller sent direct SIGTERM")

    # systemctl always addresses the real machine system manager, not a fake
    # local socket. Never allow a graphical/polkit prompt or an indefinite wait.
    for operation in ("stop", "restart", "kill"):
        result = subprocess.run(
            ["systemctl", "--system", "--no-pager", "--no-ask-password",
             operation, unit],
            input="", capture_output=True, text=True, timeout=7,
            check=False, cwd="/tmp",
        )
        if result.returncode == 0:
            raise AssertionError(f"untrusted systemctl {operation} succeeded")
        print(f"DENIED_SYSTEMCTL_{operation.upper()}", flush=True)

    try:
        with open(protected_path, "ab") as handle:
            handle.write(b"malicious update")
    except OSError as error:
        if error.errno not in (errno.EACCES, errno.EPERM):
            raise
        print("DENIED_REGISTRY_WRITE", flush=True)
    else:
        raise AssertionError("untrusted caller modified protected file")


def ambient_sudo_sibling(unit: str, expected_uid: int) -> None:
    """Demonstrate that broad runner sudo is shared by *all* same-UID code.

    This is a deliberately unsafe research role: it terminates only the
    uniquely named disposable fixture, never a real installed system service.
    """
    if not NAME.fullmatch(unit):
        raise AssertionError("refuse non-research unit")
    if os.geteuid() != expected_uid or os.geteuid() == 0:
        raise AssertionError("sibling did not retain the ordinary operator UID")
    with open("/proc/self/status", encoding="utf-8") as info:
        caps = next(line for line in info if line.startswith("CapEff:"))
    if int(caps.split(":", 1)[1], 16):
        raise AssertionError("sibling unexpectedly has effective capabilities")
    # This separate, unprivileged process uses the identical runner-wide
    # sudo right as the purported trusted operator; there is no client
    # attestation or permission separation by code/process identity.
    checked(sudo("systemctl", "--system", "--no-pager", "stop", unit))
    print("RESEARCH_MANAGER_SUDO_ENABLED_SAME_UID_SIBLING_CAN_STOP", flush=True)


def runner() -> None:
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("requires Linux unprivileged CI coordinator")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("requires actual systemd as PID1")
    if os.geteuid() == CALLER_UID:
        raise AssertionError("runner account collides with disposable principals")
    checked(sudo("true"))
    name = UNIT_PREFIX + uuid.uuid4().hex[:12] + ".service"
    root = Path(tempfile.mkdtemp(prefix="riesel-59-system-owner-"))
    root.chmod(0o755)
    script = root / "probe.py"
    shutil.copyfile(Path(__file__).resolve(), script)
    script.chmod(0o644)
    protected = root / "service-owned"
    registry = protected / "registration"
    try:
        checked(sudo("install", "-d", "-m", "0700", str(protected)))
        checked(sudo("install", "-m", "0600", "/dev/null", str(registry)))

        launch = subprocess.run(
            [PYTHON, str(script), "--launch-crash", name],
            timeout=20, capture_output=True, text=True, check=False, cwd="/tmp"
        )
        if launch.returncode != 23:
            raise AssertionError(
                f"launcher exit {launch.returncode}, expected 23: {launch.stderr!r}"
            )
        baseline = None
        for _ in range(70):
            observed = show(name)
            if observed.get("ActiveState") == "active" and int(observed.get("MainPID", "0")) > 1:
                baseline = observed
                break
            time.sleep(0.1)
        if baseline is None:
            raise AssertionError(f"system service never became active: {show(name)}")
        pid = int(baseline["MainPID"])
        service_uid = read_uid(pid)
        if service_uid in (0, CALLER_UID, os.geteuid()):
            raise AssertionError(f"transient service not isolated: UID={service_uid}")
        if not baseline.get("User", "").startswith("rieselpr"):
            raise AssertionError(f"systemd dynamic service user mismatched: {baseline}")
        if not baseline.get("InvocationID"):
            raise AssertionError("missing immutable OS attempt observation")
        print("RESEARCH_MANAGER_SYSTEM_OWNER_SURVIVES_CONTROLLER_EXIT", flush=True)

        # Two independent processes under the *same untrusted caller UID*.
        # This proves both lack direct OS rights; it does NOT establish an
        # authorized same-UID operator channel (remaining M-S2 gate).
        for idx in range(2):
            invocation = checked(
                sudo("setpriv", f"--reuid={CALLER_UID}", f"--regid={CALLER_UID}",
                     "--clear-groups", PYTHON, str(script), "--attack", name,
                     str(pid), str(registry)),
                timeout=33
            )
            for marker in ("DENIED_SIGNAL", "DENIED_SYSTEMCTL_STOP",
                           "DENIED_SYSTEMCTL_RESTART", "DENIED_SYSTEMCTL_KILL",
                           "DENIED_REGISTRY_WRITE"):
                if marker not in invocation.stdout:
                    raise AssertionError(f"caller {idx} missing {marker}: {invocation.stdout}")
            current = show(name)
            if current.get("MainPID") != str(pid) or current.get("InvocationID") != baseline["InvocationID"]:
                raise AssertionError(f"untrusted actor changed protected service: {current}")
            if current.get("ActiveState") != "active":
                raise AssertionError("protected service stopped under hostile operation")
        print("RESEARCH_MANAGER_SYSTEM_OWNER_DENIES_SAME_UID_UNTRUSTED_CALLERS", flush=True)
        if checked(sudo("cat", str(registry))).stdout:
            raise AssertionError("protected registration changed")
        print("RESEARCH_MANAGER_ROOT_OWNED_REGISTRY_UNMODIFIED", flush=True)

        # Negative AUTHENTICATION CONTROL: another independent process
        # running under the original operator's *same non-root UID* can
        # also call the CI account's ambient passwordless sudo. This is not
        # an approved Manager caller-auth mechanism. It stops only our
        # uniquely named disposable system service.
        sibling = checked([PYTHON, str(script), "--ambient-sibling", name,
                           str(os.geteuid())])
        if "RESEARCH_MANAGER_SUDO_ENABLED_SAME_UID_SIBLING_CAN_STOP" not in sibling.stdout:
            raise AssertionError(f"missing ambient sudo bypass marker: {sibling.stdout}")
        stopped = show(name)
        if stopped.get("ActiveState") != "inactive":
            raise AssertionError(f"sudo-capable sibling stop not effective: {stopped}")
        print("RESEARCH_MANAGER_SUDO_ENABLED_SAME_UID_SIBLING_CAN_STOP", flush=True)
        print("RESEARCH_MANAGER_PRIVILEGED_SYSTEM_OWNER_STOP_CONFIRMED", flush=True)
        print("RESEARCH_MANAGER_SYSTEM_OWNER_S1_DIFFERENTIAL_PASS", flush=True)
    finally:
        if NAME.fullmatch(name):
            subprocess.run(sudo("systemctl", "--system", "--no-pager", "stop", name),
                           capture_output=True, timeout=10, check=False)
            subprocess.run(sudo("systemctl", "--system", "--no-pager", "reset-failed", name),
                           capture_output=True, timeout=10, check=False)
        # Only delete our own temporary root-owned fixture; no system files.
        subprocess.run(sudo("rm", "-rf", "--", str(protected)),
                       capture_output=True, timeout=10, check=False)
        shutil.rmtree(root)


if __name__ == "__main__":
    if len(sys.argv) == 1:
        runner()
    elif len(sys.argv) == 3 and sys.argv[1] == "--launch-crash":
        launch_and_crash(sys.argv[2])
    elif len(sys.argv) == 5 and sys.argv[1] == "--attack":
        attack(sys.argv[2], int(sys.argv[3]), sys.argv[4])
    elif len(sys.argv) == 4 and sys.argv[1] == "--ambient-sibling":
        ambient_sudo_sibling(sys.argv[2], int(sys.argv[3]))
    else:
        raise SystemExit("invalid probe mode")
