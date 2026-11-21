#!/usr/bin/env python3
"""#59/#60 research-only: genuine Riesel Counter + native GUI under system OS owner.

Reuses PR #90 at an exact pinned SHA, without copying or modifying the GUI.
Exercises GUI operations against real Riesel app process run under a distinct
transient DynamicUser system service, controller exit and unprivileged OS stop.
The exported loopback TCP RPC is DELIBERATELY UNAUTHENTICATED: an arbitrary
same-user GUI invocation also mutates state; do not claim client security.

All OS changes are disposable: a uniquely named transient systemd unit, and
a uniquely named root-owned temporary directory containing one built binary.
No installed Manager, permanent account, credentials, policy or real app data.
"""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time
import uuid

PINNED_NATIVE_GUI_HEAD = "5201e61cc74bab778dfc63a3788f1708b2530f21"
ROOT_PREFIX = "/opt/riesel-m59-native-owner-"
UNIT_PREFIX = "riesel-m59-native-owner-"


def safe_root(root: Path) -> bool:
    return bool(re.fullmatch(r"/opt/riesel-m59-native-owner-[a-f0-9]{12}", str(root)))


def safe_unit(unit: str) -> bool:
    return bool(re.fullmatch(r"riesel-m59-native-owner-[a-f0-9]{12}\.service", unit))


def invoke(args, *, timeout=15, check=True):
    result = subprocess.run(
        [str(x) for x in args], stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        cwd="/tmp", timeout=timeout, check=False,
    )
    if check and result.returncode:
        raise AssertionError(
            f"failed {[str(x) for x in args[:4]]!r}, code={result.returncode}, "
            f"out={result.stdout[-650:]!r} err={result.stderr[-1100:]!r}"
        )
    return result


def sudo(*args):
    return ["sudo", "-n", *args]


def os_service(unit, *args):
    if not safe_unit(unit):
        raise AssertionError("refuse to address non-research systemd service")
    return invoke(sudo("systemctl", "--system", "--no-pager", *args, unit))


def observed_service(unit):
    if not safe_unit(unit):
        raise AssertionError("nonresearch unit")
    r = invoke(sudo("systemctl", "--system", "--no-pager", "show", unit,
                    "--property=MainPID", "--property=ActiveState",
                    "--property=ControlGroup", "--property=InvocationID",
                    "--property=User", "--property=Result"))
    return dict(line.split("=", 1) for line in r.stdout.splitlines() if "=" in line)


def journal(unit):
    if not safe_unit(unit):
        raise AssertionError("nonresearch unit")
    return invoke(sudo("journalctl", "--system", "--no-pager",
                       "-u", unit, "-o", "cat"), timeout=12).stdout


def kernel_uid(pid):
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        if line.startswith("Uid:"):
            return int(line.split()[1])
    raise AssertionError("missing kernel UID")


def actual_app_pid(path: Path):
    # The systemd MainPID is the shell wrapper, not the actual Counter.
    # Ensure that the independently owned *Rust app* was discovered.
    matched = []
    for status in Path("/proc").iterdir():
        if not status.name.isdecimal():
            continue
        try:
            raw = (status / "cmdline").read_bytes().split(b"\0", 1)[0]
        except (OSError, ProcessLookupError):
            continue
        if raw == os.fsencode(path):
            matched.append(int(status.name))
    if len(matched) != 1:
        raise AssertionError(
            f"expected exactly one native Riesel app in system service, got {matched}"
        )
    return matched[0]


def controller_launch(unit: str, root: Path):
    if not safe_unit(unit) or not safe_root(root):
        raise AssertionError("unsafe launch fixture identifier")
    binary = root / "counter_resolved_facade_host"
    user = "rieselg" + unit[len(UNIT_PREFIX):len(UNIT_PREFIX) + 9]
    # The disposable root-owned executable is under /opt, away from
    # systemd DynamicUser private /tmp and the tested /run execution denial.
    # On exact CI heads: /tmp was hidden (ENOENT), then /run execution was
    # denied (EACCES). Neither failure is a security proof about Riesel.
    # Keep the unchanged PR #90 stdin-driven research host from exiting on
    # stdin EOF. A test-only shell pipe holds stdin open without sending
    # STOP; systemd still owns the app and its child cgroup. The final
    # systemd stop therefore does NOT prove app-level graceful shutdown.
    shell = f"sleep 600 | {binary}"
    invoke(sudo("systemd-run", "--system", "--no-ask-password",
                "--unit", unit, "--property=Type=exec",
                "--property=Restart=no", "--property=DynamicUser=yes",
                "--property=User=" + user,
                "--property=KillMode=control-group",
                "--property=TimeoutStopSec=4s",
                "--property=WorkingDirectory=/tmp",
                "/bin/sh", "-c", shell))
    # Controller that requested start dies unexpectedly. Systemd outlives it.
    os._exit(23)


def assert_target_unchanged(unit, pid, baseline):
    current = observed_service(unit)
    if current["ActiveState"] != "active":
        raise AssertionError(f"service no longer active: {current}")
    if current["InvocationID"] != baseline["InvocationID"]:
        raise AssertionError("managed app attempt replaced by attacker")
    if current["MainPID"] != baseline["MainPID"]:
        raise AssertionError("OS owner process replaced by attacker")
    if not Path(f"/proc/{pid}").exists():
        raise AssertionError("native Riesel app was stopped")
    return current


def main(repo_root: Path):
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("requires disposable Linux nonroot test coordinator")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("systemd PID1 required for real OS owner test")
    invoke(sudo("true"))
    if not os.environ.get("DISPLAY") or not shutil.which("xdotool"):
        raise AssertionError("real X11 DISPLAY and xdotool required")
    repo_root = repo_root.resolve()
    actual_sha = invoke(["git", "-C", repo_root, "rev-parse", "HEAD"]).stdout.strip()
    if actual_sha != PINNED_NATIVE_GUI_HEAD:
        raise AssertionError(
            f"native GUI source not pinned: {actual_sha} != {PINNED_NATIVE_GUI_HEAD}"
        )
    backend = repo_root / "target/debug/counter_resolved_facade_host"
    gui_exe = repo_root / "research/native-gui-host/target/debug/riesel-native-gui-host-research"
    if not backend.is_file() or not gui_exe.is_file():
        raise AssertionError("original #90 app + GUI binaries not built")
    nonce = uuid.uuid4().hex[:12]
    root = Path(ROOT_PREFIX + nonce)
    unit = UNIT_PREFIX + nonce + ".service"
    if not safe_root(root) or not safe_unit(unit):
        raise AssertionError("invalid disposable identifiers")
    created = False
    started = False
    fixture = None
    try:
        # Do not alter the pinned native GUI/workspace. Only copy its actual
        # built Counter binary into a root-owned, executable temporary path
        # traversable by systemd's newly created DynamicUser app principal.
        invoke(sudo("mkdir", "--", str(root)))
        created = True
        invoke(sudo("chmod", "0755", str(root)))
        installed = root / "counter_resolved_facade_host"
        invoke(sudo("install", "-o", "root", "-g", "root",
                    "-m", "0755", str(backend), str(installed)))
        st = installed.stat()
        if st.st_uid != 0 or st.st_mode & 0o022:
            raise AssertionError("system-owned app executable must not be user writable")

        start = invoke([sys.executable, str(Path(__file__).resolve()),
                        "--launch-crash", unit, str(root)], check=False, timeout=25)
        started = True  # cleanup must run even if service launch partially failed
        if start.returncode != 23:
            raise AssertionError(
                f"launching controller did not exit 23: {start.returncode}; "
                f"{start.stdout[-500:]!r}, {start.stderr[-850:]!r}"
            )
        baseline = None
        bound = None
        for _ in range(150):
            observation = observed_service(unit)
            recent = journal(unit)
            matches = re.findall(r"RESEARCH_FACADE_BOUND (127\.0\.0\.1:\d+)", recent)
            if matches and observation.get("ActiveState") == "active":
                bound = matches[-1]
                baseline = observation
                break
            time.sleep(0.2)
        if baseline is None or not bound:
            raise AssertionError(f"real Counter did not bind: "
                                 f"owner={observed_service(unit)}, "
                                 f"journal={journal(unit)[-1400:]!r}")

        pid = actual_app_pid(installed)
        observed_uid = kernel_uid(pid)
        if observed_uid in (0, os.geteuid()):
            raise AssertionError(f"app not under isolated system UID: {observed_uid}")
        if int(baseline.get("MainPID", "0")) <= 1 or not baseline["InvocationID"]:
            raise AssertionError(f"missing actual OS owner attempt: {baseline}")
        if baseline.get("User", "").startswith("rieselg") is False:
            raise AssertionError(f"OS owner UID not registered: {baseline}")
        print("RESEARCH_M59_REAL_RIESEL_APP_SYSTEM_OWNER_SURVIVED_LAUNCHER_EXIT", flush=True)
        print(f"RESEARCH_M59_REAL_COUNTER_APP_PID={pid} UID={observed_uid}", flush=True)

        # Direct operator-account Linux OS stop attempt: same human UID as
        # native GUI, but process target isolated under a different service
        # principal. This is NOT protection against arbitrary sudo-capable
        # programs, as previously proved by #123.
        caps = next(x for x in Path("/proc/self/status").read_text().splitlines()
                    if x.startswith("CapEff:"))
        if int(caps.split(":", 1)[1], 16):
            raise AssertionError("attacking desktop operator unexpectedly has capabilities")
        try:
            os.kill(pid, signal.SIGTERM)
        except PermissionError:
            print("RESEARCH_M59_SAME_DESKTOP_ACCOUNT_DIRECT_APP_SIGTERM_DENIED", flush=True)
        else:
            raise AssertionError("unprivileged desktop user terminated protected app")
        denied = invoke(["systemctl", "--system", "--no-pager", "--no-ask-password",
                         "stop", unit], check=False, timeout=9)
        if denied.returncode == 0:
            raise AssertionError("ordinary non-sudo user stopped system owner")
        assert_target_unchanged(unit, pid, baseline)
        print("RESEARCH_M59_SAME_DESKTOP_ACCOUNT_SYSTEM_SERVICE_STOP_DENIED",
              flush=True)

        # Reuse the existing #90 Python driver and actual independent eframe
        # GUI binary, with its tested X11 keyboard and UI event loop. No GUI
        # code or public Riesel interface is reimplemented.
        driver = repo_root / "research/native-gui-host/exercise.py"
        spec = importlib.util.spec_from_file_location("riesel_gui_existing", driver)
        if spec is None or spec.loader is None:
            raise AssertionError("missing pinned GUI driver")
        fixture = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(fixture)

        legitimate = fixture.gui("protected-legitimate", bound)
        legitimate.until("GUI_VALUE 0", 22)
        window = fixture.find_window()
        fixture.focus(window)
        for value in (1, 2, 3):
            fixture.key("ctrl+i")
            legitimate.until(f"GUI_VALUE {value}", 18)

        # Adversarial desktop input boundary: a distinct, non-elevated
        # executable under the GUI's *same* X11 user sends real keyboard
        # events into that already authorized GUI window. It is not given
        # the app RPC endpoint, app capability or any Manager credential.
        # This demonstrates why GUI client authentication alone cannot
        # certify the origin of the human/user action on an X11 desktop.
        attacker_path = Path(__file__).with_name("manager_x11_same_uid_input_probe.py")
        if not attacker_path.is_file():
            raise AssertionError("missing bounded X11 attacker fixture")
        injected = invoke([sys.executable, str(attacker_path),
                           window, str(legitimate.proc.pid)], timeout=15)
        if "RESEARCH_M59_SAME_UID_X11_CLIENT_INJECTED_GUI_INCREMENT" not in injected.stdout:
            raise AssertionError(f"missing independent X11 injection marker: {injected.stdout!r}")
        legitimate.until("GUI_VALUE 4", 18)
        print("RESEARCH_M59_X11_SAME_UID_UNAUTHORIZED_GUI_INPUT_MUTATED_COUNTER",
              flush=True)
        fixture.close(legitimate, window)
        assert_target_unchanged(unit, pid, baseline)
        print("RESEARCH_M59_REAL_NATIVE_GUI_KEYBOARD_RPC_TO_SYSTEM_OWNER_PASS",
              flush=True)
        print("RESEARCH_M59_NATIVE_GUI_CLOSE_PRESERVES_SYSTEM_OWNER_PASS", flush=True)

        # An independent GUI process under exactly the *same desktop UID*
        # can attach to the same unprotected loopback API and mutate state
        # by reusing the published address. This is a demonstrated missing
        # exported-capability admission gate, NOT authorized Manager access.
        hostile = fixture.gui("untrusted-same-user-gui", bound)
        hostile.until("GUI_VALUE 4", 22)
        window2 = fixture.find_window()
        fixture.focus(window2)
        for value in (5, 6):
            fixture.key("ctrl+i")
            hostile.until(f"GUI_VALUE {value}", 18)
        fixture.close(hostile, window2)
        assert_target_unchanged(unit, pid, baseline)
        print("RESEARCH_M59_UNAUTHENTICATED_SAME_UID_GUI_CAN_MUTATE_COUNTER",
              flush=True)

        # Positive OS-owner stop control: verify the exact recorded service
        # transitions to inactive and the actual Riesel app exits. This is
        # NOT a graceful-drain proof: the pinned host's STOP-on-stdin path
        # is not reached by a systemd cgroup stop in this fixture.
        os_service(unit, "stop")
        after_stop = observed_service(unit)
        if after_stop.get("ActiveState") != "inactive":
            raise AssertionError(f"privileged stop not effective: {after_stop}")
        for _ in range(20):
            if not Path(f"/proc/{pid}").exists():
                break
            time.sleep(0.1)
        else:
            raise AssertionError("actual Riesel Counter process survived OS service stop")
        print("RESEARCH_M59_OS_OWNER_POSITIVE_STOP_ACTUAL_RIESEL_APP_PASS",
              flush=True)
        print("RESEARCH_M59_PROTECTED_SYSTEM_APP_NATIVE_GUI_INTEGRATION_PASS",
              flush=True)
    finally:
        if fixture is not None:
            for process in fixture.PROCESSES:
                if process.poll() is None:
                    process.kill()
            for process in fixture.PROCESSES:
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
        if started and safe_unit(unit):
            invoke(sudo("systemctl", "--system", "--no-pager",
                        "stop", unit), check=False, timeout=15)
            invoke(sudo("systemctl", "--system", "--no-pager",
                        "reset-failed", unit), check=False, timeout=15)
        if created and safe_root(root):
            removed = invoke(sudo("rm", "-rf", "--", str(root)), check=False)
            if removed.returncode or root.exists():
                raise AssertionError(f"temporary root fixture not removed: {removed.stderr}")


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--exercise":
        main(Path(sys.argv[2]))
    elif len(sys.argv) == 4 and sys.argv[1] == "--launch-crash":
        controller_launch(sys.argv[2], Path(sys.argv[3]))
    else:
        raise SystemExit("usage: python probe.py --exercise <pinned-PR90-tree>")
