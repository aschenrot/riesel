#!/usr/bin/env python3
"""#59/#60/#75 research: real native GUI in TWO PAM/logind sessions.

Reuses pinned and UNMODIFIED PR #90 Counter + eframe native GUI and the
separately verified real logind helper from PR #145. Both independently
running GUIs have the SAME login UID but different genuine session scopes.
A protected differently-owned Counter is bound ONLY to session A.

This test intentionally reveals the existing UNAUTHENTICATED loopback
Counter endpoint to each GUI, then verifies GUI B can mutate A's app.
That is a NEGATIVE app-export admission result, not a Manager OS-control
bypass or production trusted-desktop test. Xvfb shared X11 display is
explicitly not an actual display-manager or physical-seat login.
"""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import pwd
import re
import subprocess
import sys
import time
import uuid

COUNTER_SHA = "5201e61cc74bab778dfc63a3788f1708b2530f21"
SESSION_SHA = "c59fb50b398b16cae47de4a616ca2fc486df38b3"
PREFIX = "/opt/riesel-m59-crossgui-"


def invoke(args, *, check=True, timeout=16):
    proc = subprocess.run(
        [str(a) for a in args], input="", text=True, capture_output=True,
        cwd="/tmp", timeout=timeout, check=False,
    )
    if check and proc.returncode:
        raise AssertionError(
            f"research tool failed {args[:5]!r}, rc={proc.returncode}, "
            f"stderr={proc.stderr[-900:]!r}, stdout={proc.stdout[-400:]!r}"
        )
    return proc


def root(*args):
    return ["sudo", "-n", *args]


def load_session_module(session_root):
    path = session_root / "research/process-ownership/manager_session_bound_owner_probe.py"
    if not path.is_file():
        raise AssertionError("pinned independently verified session helper missing")
    spec = importlib.util.spec_from_file_location("session_owner_pinned", path)
    if spec is None or spec.loader is None:
        raise AssertionError("session research helper import failure")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def safe_dir(path):
    if not re.fullmatch(r"/opt/riesel-m59-crossgui-[a-f0-9]{12}", str(path)):
        raise AssertionError("refusing nonresearch installation path")
    return path


def inspect_pid(pid):
    p = Path(f"/proc/{pid}")
    raw = (p / "stat").read_text()
    fields = raw[raw.rfind(")") + 2:].split()
    status = (p / "status").read_text()
    uid = int(next(x for x in status.splitlines()
                   if x.startswith("Uid:")).split()[1])
    groups = (p / "cgroup").read_text()
    return {"uid": uid, "tick": int(fields[19]), "state": fields[0],
            "groups": groups}


def wait_marker(log, marker, *, timeout=30):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if log.exists() and marker in log.read_text(errors="replace"):
            return log.read_text(errors="replace")
        time.sleep(0.12)
    raise AssertionError(
        f"expected real GUI/backend output {marker!r}; "
        f"tail={(log.read_text(errors='replace') if log.exists() else '<absent>')[-900:]!r}"
    )


def gui_main_pid(session_mod, unit, log, uid, scope, executable):
    wait_marker(log, "GUI_PID ", timeout=30)
    match = re.search(r"(?m)^GUI_PID ([0-9]+)$", log.read_text())
    if match is None:
        raise AssertionError("real GUI_PID log invalid")
    pid = int(match.group(1))
    observed = inspect_pid(pid)
    if (observed["uid"] != uid or observed["state"] in ("Z", "X") or
            scope not in observed["groups"]):
        raise AssertionError(
            f"real native GUI not in expected PAM session scope: "
            f"uid={observed['uid']} expected={uid}, cgroup={observed['groups']!r}, scope={scope}"
        )
    unit_state = session_mod.service_state(unit)
    if unit_state.get("ActiveState") != "active":
        raise AssertionError(f"real PAM native GUI service not active: {unit_state}")
    proc_exe = os.readlink(f"/proc/{pid}/exe")
    if proc_exe != str(executable):
        raise AssertionError(f"GUI executable differs from pinned installed GUI: {proc_exe}")
    return pid, observed["tick"]


def window_for(pid, *, timeout=20):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        found = invoke(
            ["xdotool", "search", "--onlyvisible", "--name",
             "Riesel native GUI research"], check=False
        )
        if found.returncode == 0:
            for candidate in found.stdout.splitlines():
                if not candidate.strip().isdecimal():
                    continue
                actual = invoke(["xdotool", "getwindowpid", candidate.strip()],
                                check=False)
                if actual.returncode == 0 and actual.stdout.strip() == str(pid):
                    return candidate.strip()
        time.sleep(0.15)
    raise AssertionError(f"no actual X11 native GUI window for PID {pid}")


def gui_key(window, key):
    invoke(["xdotool", "windowfocus", "--sync", window])
    invoke(["xdotool", "key", "--clearmodifiers", key])


def wait_session(session_mod, account, expected_other=None, *, timeout=27):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        sessions = session_mod.sessions_for(account)
        other = [sid for sid in sessions if sid != expected_other]
        if len(other) == 1:
            attrs = session_mod.session_state(other[0])
            if (attrs.get("Service") == "login" and attrs.get("Scope")
                    and attrs.get("Name") == account):
                scope = session_mod.scope_safe(attrs["Scope"])
                if expected_other:
                    other_scope = session_mod.session_state(expected_other)["Scope"]
                    if other_scope == scope:
                        raise AssertionError("two sessions share one scope")
                return other[0], scope
        time.sleep(0.12)
    raise AssertionError(
        f"actual independent PAM/logind session not found: "
        f"{session_mod.sessions_for(account)}"
    )


def start_gui_session(session_mod, unit, user, directory, gui_log, display):
    session_mod.unit_safe(unit)
    # Run the real GUI as the main process of a real PAM/logind session.
    # The launcher initially waits for the exact root-published research
    # Counter address. Its execv later keeps the same PID/session.
    return invoke(root(
        "systemd-run", "--system", "--no-block", "--no-ask-password",
        "--unit", unit, "--property=Type=exec", "--property=Restart=no",
        "--property=User=" + user, "--property=PAMName=login",
        "--property=WorkingDirectory=/tmp",
        "--property=Environment=DISPLAY=" + display,
        "--property=Environment=HOME=/tmp",
        "--property=Environment=LIBGL_ALWAYS_SOFTWARE=1",
        "--property=Environment=GALLIUM_DRIVER=llvmpipe",
        "--property=KillMode=control-group",
        "--property=StandardOutput=append:" + str(gui_log),
        "--property=StandardError=append:" + str(gui_log),
        "/usr/bin/python3", str(directory / "launcher.py"), str(directory)
    ))


def observe_endpoint(log):
    for _ in range(180):
        if log.exists():
            text = log.read_text(errors="replace")
            match = re.search(r"RESEARCH_FACADE_BOUND (127\.0\.0\.1:[0-9]+)", text)
            if match:
                return match.group(1)
        time.sleep(0.12)
    raise AssertionError(
        f"real Riesel Counter did not advertise test RPC endpoint: "
        f"{log.read_text(errors='replace')[-850:] if log.exists() else 'no log'}"
    )


def exercise(counter_root, session_root, launcher_source):
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("disposable nonroot Linux CI coordinator required")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("actual systemd PID 1 required")
    display = os.environ.get("DISPLAY", "")
    if not display:
        raise AssertionError("Xvfb display missing")
    for tool in ("xdotool", "xhost", "setpriv", "loginctl"):
        if invoke(["/usr/bin/which", tool], check=False).returncode:
            raise AssertionError(f"missing required real Linux/X11 tool: {tool}")
    for path, digest in ((counter_root, COUNTER_SHA),
                         (session_root, SESSION_SHA)):
        rev = invoke(["git", "-C", str(path), "rev-parse", "HEAD"]).stdout.strip()
        if rev != digest:
            raise AssertionError(f"unexpected prior research source {path}: {rev}")
    session_mod = load_session_module(session_root)
    backend_src = counter_root / "target/debug/counter_resolved_facade_host"
    gui_src = (counter_root / "research/native-gui-host/target/debug/"
               "riesel-native-gui-host-research")
    for f in (backend_src, gui_src, launcher_source):
        if not f.is_file():
            raise AssertionError(f"missing previously pinned GUI/Counter asset: {f}")
    nonce = uuid.uuid4().hex[:12]
    directory = safe_dir(Path(PREFIX + nonce))
    account = session_mod.account_safe("rieselx" + nonce[:10])
    unit_a = session_mod.unit_safe(
        f"riesel-m59-session-{nonce}-login.service")
    unit_b = session_mod.unit_safe(
        f"riesel-m59-session-{nonce}-peerlogin.service")
    backend_unit = session_mod.unit_safe(
        f"riesel-m59-session-{nonce}-bound.service")
    backend = directory / "counter_resolved_facade_host"
    gui = directory / "riesel-native-gui-host-research"
    log_a, log_b, log_backend = (
        directory / "gui-A.log", directory / "gui-B.log",
        directory / "counter.log",
    )
    installed = False
    created_user = False
    x_permission = False
    started = []
    session_a, session_b = None, None
    try:
        if invoke(root("id", "-u", account), check=False).returncode == 0:
            raise AssertionError("generated disposable login account exists")
        invoke(root("useradd", "--no-create-home", "--user-group",
                    "--shell", "/bin/bash", account))
        created_user = True
        info = pwd.getpwnam(account)
        if info.pw_uid in (0, os.geteuid()):
            raise AssertionError("login UID collides with root/runner")
        invoke(root("install", "-d", "-o", "root", "-g", "root", "-m",
                    "0755", str(directory)))
        installed = True
        for src, target in ((backend_src, backend), (gui_src, gui),
                            (launcher_source, directory / "launcher.py")):
            invoke(root("install", "-o", "root", "-g", "root", "-m",
                        "0755", str(src), str(target)))
        # CI Xvfb belongs to runner; explicitly admit ONLY the disposable
        # user to share this X11 display. This is NOT a trustworthy desktop
        # consent isolation; X11 clients with this UID can inject input.
        invoke(["xhost", "+SI:localuser:" + account])
        x_permission = True
        print("RESEARCH_M59_XVFB_GUI_LOGIN_UID_DISPLAY_GRANTED", flush=True)

        started.append(unit_a)
        start_gui_session(session_mod, unit_a, account, directory, log_a, display)
        session_a, scope_a = wait_session(session_mod, account)
        print("RESEARCH_M59_REAL_GUI_PAM_SESSION_A_SCOPE", flush=True)

        # Start real, separately OS-owned Riesel application. It is bound
        # only to session A's exact, observed logind scope.
        started.append(backend_unit)
        invoke(root(
            "systemd-run", "--system", "--no-ask-password",
            "--unit", backend_unit, "--property=Type=exec",
            "--property=Restart=no", "--property=DynamicUser=yes",
            "--property=User=rieselg" + nonce[:9],
            "--property=KillMode=control-group",
            "--property=TimeoutStopSec=4s",
            "--property=WorkingDirectory=/tmp",
            "--property=BindsTo=" + scope_a,
            "--property=After=" + scope_a,
            "--property=StandardOutput=append:" + str(log_backend),
            "--property=StandardError=append:" + str(log_backend),
            "/bin/sh", "-c", f"sleep 600 | {backend}"
        ), timeout=22)
        backend_state = session_mod.observe_service(backend_unit, backend)
        if (backend_state["uid"] in (0, os.geteuid(), info.pw_uid)
                or scope_a not in backend_state["state"]["BindsTo"].split()):
            raise AssertionError("real Riesel backend not protected/bound to exact scope")
        print("RESEARCH_M59_REAL_COUNTER_PROTECTED_EXACT_GUI_SESSION_BOUND",
              flush=True)

        endpoint = observe_endpoint(log_backend)
        # Real app's unsecured endpoint is deliberately published as an
        # ordinary address, NOT a credential or Management authorization.
        endpoint_temp = directory / "endpoint.tmp"
        endpoint_temp.write_text(endpoint + "\n")
        invoke(root("install", "-o", "root", "-g", "root", "-m", "0644",
                    str(endpoint_temp), str(directory / "counter-endpoint")))
        endpoint_temp.unlink()
        wait_marker(log_a, "GUI_VALUE 0", timeout=35)
        pid_a, tick_a = gui_main_pid(
            session_mod, unit_a, log_a, info.pw_uid, scope_a, gui
        )
        window_a = window_for(pid_a)
        gui_key(window_a, "ctrl+i")
        wait_marker(log_a, "GUI_VALUE 1")
        print("RESEARCH_M59_REAL_SESSION_A_NATIVE_GUI_MODIFIED_COUNTER",
              flush=True)

        started.append(unit_b)
        start_gui_session(session_mod, unit_b, account, directory, log_b, display)
        session_b, scope_b = wait_session(
            session_mod, account, expected_other=session_a
        )
        if scope_b == scope_a:
            raise AssertionError("two actual PAM sessions share one scope")
        wait_marker(log_b, "GUI_VALUE 1", timeout=35)
        pid_b, tick_b = gui_main_pid(
            session_mod, unit_b, log_b, info.pw_uid, scope_b, gui
        )
        if pid_b == pid_a:
            raise AssertionError("two native GUI clients share one OS process")
        window_b = window_for(pid_b)
        gui_key(window_b, "ctrl+i")
        wait_marker(log_b, "GUI_VALUE 2")
        wait_marker(log_a, "GUI_VALUE 2")
        print("RESEARCH_M59_CROSS_LOGIND_SESSION_UNAUTHENTICATED_GUI_MUTATED_APP",
              flush=True)

        invoke(root("loginctl", "terminate-session", session_b))
        for _ in range(95):
            if not session_mod.live_same(pid_b, tick_b):
                break
            time.sleep(0.15)
        else:
            raise AssertionError("unrelated session B GUI survived its logout")
        state = session_mod.service_state(backend_unit)
        if (state.get("ActiveState") != "active"
            or state.get("InvocationID") != backend_state["invocation"]
            or not session_mod.live_same(backend_state["pid"],
                                         backend_state["tick"])
            or not session_mod.live_same(pid_a, tick_a)):
            raise AssertionError(
                "unrelated same-UID GUI session logout stopped session A app")
        gui_key(window_a, "ctrl+i")
        wait_marker(log_a, "GUI_VALUE 3")
        print("RESEARCH_M59_UNRELATED_GUI_LOGOUT_PRESERVES_APP_A", flush=True)

        invoke(root("loginctl", "terminate-session", session_a))
        for _ in range(120):
            state = session_mod.service_state(backend_unit)
            if (state.get("ActiveState") in ("inactive", "failed")
                and not session_mod.live_same(backend_state["pid"],
                                              backend_state["tick"])
                and not session_mod.live_same(pid_a, tick_a)):
                break
            time.sleep(0.15)
        else:
            raise AssertionError(
                "bound app/GUI survived exact PAM session termination: "
                + str(session_mod.service_state(backend_unit)))
        print("RESEARCH_M59_BOUND_GUI_LOGOUT_TERMINATES_EXACT_COUNTER_APP",
              flush=True)
        print("RESEARCH_M59_REAL_CROSS_SESSION_NATIVE_GUI_SECURITY_RESEARCH_PASS",
              flush=True)
    finally:
        for unit in reversed(started):
            invoke(root("systemctl", "--system", "--no-pager", "stop", unit),
                   check=False)
            invoke(root("systemctl", "--system", "--no-pager", "reset-failed", unit),
                   check=False)
        for sid in (session_b, session_a):
            if sid and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", sid):
                invoke(root("loginctl", "terminate-session", sid),
                       check=False)
        if x_permission:
            invoke(["xhost", "-SI:localuser:" + account], check=False)
        if created_user:
            invoke(root("pkill", "-KILL", "-u", account), check=False)
            for _ in range(35):
                result = invoke(root("userdel", account), check=False)
                if result.returncode == 0:
                    break
                time.sleep(0.15)
            else:
                raise AssertionError(
                    f"could not remove disposable login account: "
                    f"{result.stderr[-350:]}")
        if installed:
            invoke(root("rm", "-rf", "--", str(safe_dir(directory))),
                   check=False)


if __name__ == "__main__":
    if len(sys.argv) != 5 or sys.argv[1] != "--exercise":
        raise SystemExit(
            "usage: probe.py --exercise <pinned-PR90> <pinned-PR145> <launcher.py>"
        )
    exercise(Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve(),
             Path(sys.argv[4]).resolve())
