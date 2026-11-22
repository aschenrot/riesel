#!/usr/bin/env python3
"""#59 bounded REAL logind session / protected Riesel owner experiment.

Uses a temporary, genuine PAM/logind login session for a disposable account.
Tests separately OS-owned Riesel Counter services *with* and *without* an
explicit BindsTo=<exact session scope> dependency. The session-bound app
runs as its OWN isolated DynamicUser; the login user/attacker does not own it.

This is Linux systemd research. A PAM 'login' session on CI is not a
graphical desktop/Wayland seat. It is NOT a production Manager, trusted
human-authorization proof, secure GUI IPC or a Windows/macOS guarantee.
"""
from __future__ import annotations

import os
from pathlib import Path
import pwd
import re
import signal
import stat
import subprocess
import sys
import tempfile
import time
import uuid

PINNED = "5201e61cc74bab778dfc63a3788f1708b2530f21"
APP_PREFIX = "/opt/riesel-m59-session-owner-"
UNIT_PREFIX = "riesel-m59-session-"
REGISTRY_PREFIX = "riesel-m59-session-registry-"
ACCOUNT_PREFIX = "rieselx"


def run(argv, *, check=True, timeout=15):
    p = subprocess.run([str(a) for a in argv], cwd="/tmp",
                       input="", text=True, capture_output=True,
                       timeout=timeout, check=False)
    if check and p.returncode:
        raise AssertionError(
            f"command failed rc={p.returncode}: {argv[:4]!r}; "
            f"stderr={p.stderr[-850:]!r}; stdout={p.stdout[-350:]!r}")
    return p


def root(*args):
    return ["sudo", "-n", *args]


def unit_safe(value):
    if not re.fullmatch(
        r"riesel-m59-session-[a-f0-9]{12}-(login|peerlogin|bound|machine)\.service",
        value
    ):
        raise AssertionError("refusing nonresearch systemd unit")
    return value


def root_safe(path):
    if not re.fullmatch(r"/opt/riesel-m59-session-owner-[a-f0-9]{12}",
                        str(path)):
        raise AssertionError("refusing nonresearch installation path")
    return path


def scope_safe(value):
    if not re.fullmatch(r"session-[A-Za-z0-9_-]+\.scope", value):
        raise AssertionError("logind did not report a real session scope")
    return value


def account_safe(value):
    if not re.fullmatch(r"rieselx[a-f0-9]{10}", value):
        raise AssertionError("refusing nonresearch temporary account")
    return value


def service_state(unit):
    unit_safe(unit)
    p = run(["systemctl", "--system", "--no-pager", "--no-ask-password",
             "show", unit, "--property=MainPID", "--property=InvocationID",
             "--property=ActiveState", "--property=BindsTo",
             "--property=After", "--property=Result"])
    return dict(line.split("=", 1) for line in p.stdout.splitlines()
                if "=" in line)


def sessions_for(account):
    account_safe(account)
    result = run(["loginctl", "list-sessions", "--no-legend"])
    hits = []
    for line in result.stdout.splitlines():
        columns = line.strip().split()
        if len(columns) >= 3 and columns[2] == account:
            hits.append(columns[0])
    return hits


def session_state(session):
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", session):
        raise AssertionError("untrusted session identifier format")
    result = run(["loginctl", "show-session", session, "--property=User",
                  "--property=Name", "--property=Scope", "--property=State",
                  "--property=Service", "--property=Class"])
    return dict(line.split("=", 1) for line in result.stdout.splitlines()
                if "=" in line)


def actual_pid(executable):
    candidates = []
    for entry in Path("/proc").iterdir():
        if not entry.name.isdecimal():
            continue
        try:
            cmd = (entry / "cmdline").read_bytes().split(b"\0", 1)[0]
        except (PermissionError, FileNotFoundError, ProcessLookupError):
            continue
        if cmd == os.fsencode(executable):
            candidates.append(int(entry.name))
    if len(candidates) != 1:
        raise AssertionError(
            f"expected one real Riesel Counter for {executable}: {candidates}")
    return candidates[0]


def proc_fields(pid):
    data = Path(f"/proc/{pid}/stat").read_text()
    fields = data[data.rfind(")") + 2:].split()
    status = Path(f"/proc/{pid}/status").read_text()
    uid = int(next(x for x in status.splitlines()
                   if x.startswith("Uid:")).split()[1])
    return {"state": fields[0], "tick": int(fields[19]), "uid": uid}


def live_same(pid, tick):
    try:
        st = proc_fields(pid)
        return st["tick"] == tick and st["state"] not in ("Z", "X")
    except (OSError, ProcessLookupError):
        return False


def observe_service(unit, executable):
    for _ in range(110):
        state = service_state(unit)
        if (state.get("ActiveState") == "active" and
                state.get("InvocationID") and
                int(state.get("MainPID", "0")) > 1):
            try:
                pid = actual_pid(executable)
                actual = proc_fields(pid)
            except (AssertionError, OSError):
                time.sleep(0.2)
                continue
            return {"unit": unit, "invocation": state["InvocationID"],
                    "pid": pid, "tick": actual["tick"],
                    "uid": actual["uid"], "state": state}
        time.sleep(0.2)
    raise AssertionError(f"real Riesel service not observed: {service_state(unit)}")


def attacker(pid, unit, registry):
    if os.geteuid() == 0 or os.geteuid() != os.getuid():
        raise AssertionError("attacker must be ordinary nonroot login user")
    status = Path("/proc/self/status").read_text()
    caps = int(next(line.split(":")[1].strip() for line in status.splitlines()
                    if line.startswith("CapEff:")), 16)
    if caps:
        raise AssertionError("untrusted login subject has effective caps")
    if run(["sudo", "-n", "true"], check=False).returncode == 0:
        raise AssertionError("untrusted account unexpectedly has sudo")
    try:
        Path(registry).read_bytes()
    except PermissionError:
        pass
    else:
        raise AssertionError("untrusted login UID read protected registration")
    try:
        Path(registry).write_bytes(b"forged")
    except PermissionError:
        pass
    else:
        raise AssertionError("untrusted login UID edited protected registration")
    if run(["systemctl", "--system", "--no-ask-password", "stop", unit],
           check=False).returncode == 0:
        raise AssertionError("login account stopped protected system owner")
    try:
        os.kill(pid, signal.SIGTERM)
    except PermissionError:
        pass
    else:
        raise AssertionError("untrusted login UID sent SIGTERM to app")
    print("RESEARCH_M59_SESSION_SAME_LOGIN_UID_DIRECT_MUTATION_DENIED",
          flush=True)


def main(pinned):
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("needs ordinary Ubuntu Linux coordinator")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("needs a real systemd PID 1")
    for exe in ("loginctl", "systemd-run", "systemctl", "setpriv", "sudo"):
        if run(["/usr/bin/which", exe], check=False).returncode:
            raise AssertionError(f"missing actual Linux prerequisite: {exe}")
    if run(["git", "-C", str(pinned), "rev-parse",
            "HEAD"]).stdout.strip() != PINNED:
        raise AssertionError("must use exact pinned unmodified real Counter")
    built = pinned / "target/debug/counter_resolved_facade_host"
    if not built.is_file():
        raise AssertionError("real pinned Riesel binary absent")
    nonce = uuid.uuid4().hex[:12]
    account = account_safe(ACCOUNT_PREFIX + nonce[:10])
    path = root_safe(Path(APP_PREFIX + nonce))
    login_unit = unit_safe(UNIT_PREFIX + nonce + "-login.service")
    peer_login_unit = unit_safe(UNIT_PREFIX + nonce + "-peerlogin.service")
    bound_unit = unit_safe(UNIT_PREFIX + nonce + "-bound.service")
    machine_unit = unit_safe(UNIT_PREFIX + nonce + "-machine.service")
    registry = Path(tempfile.mkdtemp(prefix=REGISTRY_PREFIX))
    # Initial directory is created by runner; root takes full ownership
    # before another user can see a registration or application state.
    bound_exe = path / "bound/counter_resolved_facade_host"
    machine_exe = path / "machine/counter_resolved_facade_host"
    account_created = False
    installed = False
    session = None
    peer_session = None
    started = []
    try:
        if run(root("id", "-u", account), check=False).returncode == 0:
            raise AssertionError("generated disposable login user already exists")
        run(root("useradd", "--no-create-home", "--user-group",
                 "--shell", "/bin/bash", account_safe(account)))
        account_created = True
        info = pwd.getpwnam(account)
        if info.pw_uid in (0, os.geteuid()):
            raise AssertionError("research login principal collides")
        run(root("chown", "root:root", str(registry)))
        run(root("chmod", "0700", str(registry)))
        run(root("install", "-o", "root", "-g", "root", "-m",
                 "0600", "/dev/null", str(registry / "registration")))
        if run(root("test", "-f", str(registry / "registration")),
               check=False).returncode:
            raise AssertionError("protected registration missing")
        run(root("install", "-d", "-o", "root", "-g", "root", "-m",
                 "0755", str(path)))
        installed = True
        for name, target in (("bound", bound_exe), ("machine", machine_exe)):
            run(root("install", "-d", "-o", "root", "-g", "root", "-m",
                     "0755", str(path / name)))
            run(root("install", "-o", "root", "-g", "root", "-m",
                     "0755", str(built), str(target)))
        attacker_path = path / "attacker.py"
        run(root("install", "-o", "root", "-g", "root", "-m",
                 "0755", str(Path(__file__).resolve()), str(attacker_path)))

        if sessions_for(account):
            raise AssertionError("disposable account had unexpected prior login")
        # PAMName=login creates a real, but NON-GRAPHICAL, logind session
        # under a temporary account. No display manager/session bus is
        # inferred from the presence of a session scope.
        # No blocking wait for Type=exec/PAM session setup. systemd-run
        # may otherwise wait on the login session job for longer than the
        # caller timeout; observe the REAL logind session separately.
        started.append(login_unit)  # clean exact unit even if launch times out
        run(root("systemd-run", "--system", "--no-block",
                 "--no-ask-password",
                 "--unit", login_unit, "--property=Type=exec",
                 "--property=Restart=no", "--property=User=" + account,
                 "--property=PAMName=login",
                 "--property=WorkingDirectory=/tmp",
                 "/usr/bin/sleep", "480"))

        observed_session = None
        for _ in range(120):
            found = sessions_for(account)
            if len(found) == 1:
                session = found[0]
                attrs = session_state(session)
                if attrs.get("Scope") and attrs.get("Service") == "login":
                    observed_session = attrs
                    break
            time.sleep(0.2)
        if not observed_session:
            status = service_state(login_unit)
            diag = run(root("journalctl", "--no-pager",
                            "--unit", login_unit, "--lines", "18"),
                       check=False)
            raise AssertionError(
                f"actual PAM/logind session not established: "
                f"sessions={sessions_for(account)}; unit={status}; "
                f"journal_tail={diag.stdout[-1800:]!r}")
        scope = scope_safe(observed_session["Scope"])
        if int(observed_session["User"]) != info.pw_uid:
            raise AssertionError("session has wrong actual kernel user")
        print("RESEARCH_M59_ACTUAL_LOGIN_SESSION_SCOPE_OBSERVED", flush=True)

        # One real Riesel app follows exact login session lifetime; a
        # separate machine-background counterpart does not.
        for unit, exe, identity, extra in (
            (bound_unit, bound_exe, "rieselb" + nonce[:9],
             ["--property=BindsTo=" + scope,
              "--property=After=" + scope]),
            (machine_unit, machine_exe, "rieselm" + nonce[:9], []),
        ):
            started.append(unit)  # always clean even on delayed job failure
            run(root("systemd-run", "--system", "--no-ask-password",
                     "--unit", unit, "--property=Type=exec",
                     "--property=Restart=no", "--property=DynamicUser=yes",
                     "--property=User=" + identity,
                     "--property=KillMode=control-group",
                     "--property=TimeoutStopSec=4s",
                     "--property=WorkingDirectory=/tmp", *extra,
                     "/bin/sh", "-c", f"sleep 600 | {exe}"))
        bound = observe_service(bound_unit, bound_exe)
        machine = observe_service(machine_unit, machine_exe)
        if (len({bound["uid"], machine["uid"], info.pw_uid,
                 os.geteuid(), 0}) != 5):
            raise AssertionError("real Riesel app principal isolation failed")
        binding = bound["state"]["BindsTo"].split()
        if scope not in binding:
            raise AssertionError("session app lacks actual BindsTo=scope binding")
        if scope in machine["state"]["BindsTo"].split():
            raise AssertionError("background app unexpectedly tied to login")
        print("RESEARCH_M59_SAME_HOST_TWO_REAL_RIESEL_EXECUTION_SCOPES",
              flush=True)

        attack_result = run(root(
            "setpriv", "--reuid", str(info.pw_uid),
            "--regid", str(info.pw_gid), "--clear-groups",
            "/usr/bin/python3", str(attacker_path), "--attack",
            str(bound["pid"]), bound_unit, str(registry / "registration")),
            check=False)
        if (attack_result.returncode or
            "RESEARCH_M59_SESSION_SAME_LOGIN_UID_DIRECT_MUTATION_DENIED"
                not in attack_result.stdout):
            raise AssertionError(
                f"hostile disposable login user was not denied: "
                f"rc={attack_result.returncode}; "
                f"out={attack_result.stdout[-600:]}; "
                f"err={attack_result.stderr[-600:]}")
        print(attack_result.stdout.strip(), flush=True)
        if (service_state(bound_unit)["InvocationID"] != bound["invocation"]
            or not live_same(bound["pid"], bound["tick"])
            or service_state(machine_unit)["InvocationID"] != machine["invocation"]
            or not live_same(machine["pid"], machine["tick"])):
            raise AssertionError("unprivileged login attack changed OS attempt")
        print("RESEARCH_M59_UNTRUSTED_LOGIN_UID_UNCHANGED_ATTEMPTS",
              flush=True)

        # Second ACTUAL PAM/logind session for the SAME login UID. Ending
        # this unrelated scope must not stop app bound to the first scope.
        # This differentiates exact-session lifetime from per-user presence.
        started.append(peer_login_unit)
        run(root("systemd-run", "--system", "--no-block", "--no-ask-password",
                 "--unit", peer_login_unit, "--property=Type=exec",
                 "--property=Restart=no", "--property=User=" + account,
                 "--property=PAMName=login",
                 "--property=WorkingDirectory=/tmp",
                 "/usr/bin/sleep", "480"))
        for _ in range(120):
            all_sessions = sessions_for(account)
            extras = [x for x in all_sessions if x != session]
            if len(extras) == 1:
                candidate = extras[0]
                attrs = session_state(candidate)
                if (attrs.get("Scope") and attrs.get("Service") == "login"
                    and int(attrs.get("User", "-1")) == info.pw_uid
                    and attrs["Scope"] != scope):
                    peer_session = candidate
                    break
            time.sleep(0.2)
        if peer_session is None:
            raise AssertionError(
                f"second independent same-user PAM/logind session not observed: "
                f"current={sessions_for(account)}; "
                f"unit={service_state(peer_login_unit)}")
        peer_scope = scope_safe(session_state(peer_session)["Scope"])
        if peer_scope == scope:
            raise AssertionError("two same-user sessions reused the same scope")
        print("RESEARCH_M59_TWO_PARALLEL_SAME_USER_LOGIN_SCOPES", flush=True)

        run(root("loginctl", "terminate-session", peer_session))
        # Another session closing is not a privileged Manager stop of app A.
        if (service_state(bound_unit)["ActiveState"] != "active"
            or service_state(bound_unit)["InvocationID"] != bound["invocation"]
            or not live_same(bound["pid"], bound["tick"])
            or service_state(machine_unit)["ActiveState"] != "active"
            or service_state(machine_unit)["InvocationID"] != machine["invocation"]
            or not live_same(machine["pid"], machine["tick"])):
            raise AssertionError(
                "unrelated same-user session logout stopped protected Riesel app")
        print("RESEARCH_M59_UNRELATED_LOGIN_LOGOUT_LEAVES_BOTH_APPS_ALIVE",
              flush=True)

        # Explicit ROOT test operator terminates only the exact generated
        # PAM session. The user-session app must follow its session's
        # scope, while machine background app must NOT follow logout.
        run(root("loginctl", "terminate-session", session))
        for _ in range(120):
            state = service_state(bound_unit)
            if (state.get("ActiveState") in ("inactive", "failed") and
                not live_same(bound["pid"], bound["tick"])):
                break
            time.sleep(0.2)
        else:
            raise AssertionError(
                "session-bound real Counter did not stop after actual logout: "
                + str(service_state(bound_unit)))
        fresh_machine = service_state(machine_unit)
        if (fresh_machine.get("ActiveState") != "active" or
            fresh_machine.get("InvocationID") != machine["invocation"] or
            not live_same(machine["pid"], machine["tick"])):
            raise AssertionError(
                f"machine-service Counter incorrectly stopped on logout: {fresh_machine}")
        print("RESEARCH_M59_LOGIN_SESSION_END_STOPS_ONLY_BOUND_RIESEL_APP",
              flush=True)
        print("RESEARCH_M59_PROTECTED_SESSION_BOUND_OWNER_RESEARCH_PASS",
              flush=True)
    finally:
        for unit in reversed(started):
            run(root("systemctl", "--system", "--no-pager", "stop",
                     unit_safe(unit)), check=False)
            run(root("systemctl", "--system", "--no-pager", "reset-failed",
                     unit_safe(unit)), check=False)
        for sid in (peer_session, session):
            if sid is not None and re.fullmatch(r"[A-Za-z0-9_-]{1,64}", sid):
                # Only exact sessions observed for our random disposable user.
                run(root("loginctl", "terminate-session", sid), check=False)
        if account_created:
            # Newly created random NSS account is exclusively our fixture.
            run(root("pkill", "-KILL", "-u", account_safe(account)),
                check=False)
            for _ in range(40):
                result = run(root("userdel", account_safe(account)),
                             check=False)
                if not result.returncode:
                    break
                time.sleep(0.2)
            else:
                raise AssertionError(
                    f"failed to remove disposable user: {result.stderr[-400:]}")
        if installed:
            run(root("rm", "-rf", "--", str(root_safe(path))), check=False)
        if (registry.parent == Path("/tmp") and
            re.fullmatch(r"riesel-m59-session-registry-[a-zA-Z0-9_-]+",
                         registry.name)):
            run(root("rm", "-rf", "--", str(registry)), check=False)


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "--exercise":
        main(Path(sys.argv[2]).resolve())
    elif len(sys.argv) == 5 and sys.argv[1] == "--attack":
        attacker(int(sys.argv[2]), sys.argv[3], sys.argv[4])
    else:
        raise SystemExit("invalid protected-session research invocation")
