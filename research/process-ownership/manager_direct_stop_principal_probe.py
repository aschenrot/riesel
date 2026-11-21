#!/usr/bin/env python3
"""Research-only Linux direct-stop bypass probe.

Only targets its own disposable subprocess. Creates no users, systemd units,
service registrations or persistent files. Requires setpriv and either a
disposable root-enabled runner or passwordless sudo.
"""
import errno
import os
import select
import signal
import subprocess
import sys
import time

OWNER_UID = 60081
FOREIGN_UID = 60082


def as_user(uid, *command):
    sudo = [] if os.geteuid() == 0 else ["sudo", "-n"]
    return [*sudo, "setpriv", f"--reuid={uid}", f"--regid={uid}",
            "--clear-groups", *command]


def victim():
    if os.geteuid() != OWNER_UID or os.getegid() != OWNER_UID:
        raise SystemExit("victim did not drop to the isolated principal")
    # Print the real child PID, not a possible sudo-wrapper PID.
    print(f"READY {os.getpid()} {os.geteuid()}", flush=True)
    while True:
        time.sleep(1)


def attacker(pid):
    if os.geteuid() == 0:
        raise SystemExit("attacker still has root privileges")
    with open("/proc/self/status", encoding="utf-8") as status:
        caps = next(line for line in status if line.startswith("CapEff:"))
    if int(caps.split(":", 1)[1], 16):
        raise SystemExit("attacker retained effective capabilities")
    try:
        # Send an actual termination signal, not merely kill(pid, 0).
        os.kill(pid, signal.SIGTERM)
    except OSError as error:
        if error.errno == errno.EPERM:
            print("EPERM", flush=True)
            return
        raise
    print("DELIVERED", flush=True)


def run():
    if sys.platform != "linux":
        raise SystemExit("Linux-only research test")
    script = os.path.abspath(__file__)
    worker = subprocess.Popen(
        as_user(OWNER_UID, sys.executable, "-u", script, "--victim"),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    child_pid = None
    try:
        ready, _, _ = select.select([worker.stdout], [], [], 8)
        if not ready:
            raise AssertionError("worker did not become ready")
        line = worker.stdout.readline().strip()
        parts = line.split()
        if len(parts) != 3 or parts[0] != "READY" or int(parts[2]) != OWNER_UID:
            raise AssertionError(f"unexpected worker credentials: {line!r}")
        child_pid = int(parts[1])
        if child_pid <= 1:
            raise AssertionError("invalid child PID")
        os.kill(child_pid, 0)

        def attempt(uid):
            result = subprocess.run(
                as_user(uid, sys.executable, script, "--attack", str(child_pid)),
                capture_output=True, text=True, timeout=8, check=False,
            )
            if result.returncode:
                raise AssertionError(f"attacker failed: {result.stderr} / {result.stdout}")
            return result.stdout.strip()

        foreign_result = attempt(FOREIGN_UID)
        if foreign_result != "EPERM":
            raise AssertionError(f"foreign UID stop not denied: {foreign_result}")
        os.kill(child_pid, 0)
        print("RESEARCH_MANAGER_CROSS_UID_DIRECT_SIGTERM_DENIED", flush=True)

        same_result = attempt(OWNER_UID)
        if same_result != "DELIVERED":
            raise AssertionError(f"same-UID direct stop not delivered: {same_result}")
        worker.wait(timeout=8)
        if worker.returncode == 0:
            raise AssertionError("worker exited successfully instead of from SIGTERM")
        print("RESEARCH_MANAGER_SAME_UID_DIRECT_SIGTERM_BYPASSES_MANAGER", flush=True)
        print("RESEARCH_MANAGER_OS_SIGNAL_PRINCIPAL_DIFFERENTIAL_PASS", flush=True)
    finally:
        # Kill *only* this test's recorded child, and only if the original
        # sudo/setpriv worker has not already exited. On the hosted runner
        # use sudo because the coordinator itself is a different UID.
        if child_pid is not None and worker.poll() is None:
            sudo = [] if os.geteuid() == 0 else ["sudo", "-n"]
            subprocess.run([*sudo, "kill", "-KILL", str(child_pid)],
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                           check=False, timeout=5)
        if worker.poll() is None:
            worker.kill()
        worker.communicate(timeout=8)


if __name__ == "__main__":
    if len(sys.argv) == 2 and sys.argv[1] == "--victim":
        victim()
    elif len(sys.argv) == 3 and sys.argv[1] == "--attack":
        attacker(int(sys.argv[2]))
    elif len(sys.argv) == 1:
        run()
    else:
        raise SystemExit("invalid research mode")
