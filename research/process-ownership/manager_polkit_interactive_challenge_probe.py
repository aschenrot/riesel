#!/usr/bin/env python3
"""#59 research: real polkit challenge/response for exact trusted action details.

Disposable Ubuntu CI only. A fictitious polkit action and generated unprivileged
NSS test account are installed and always removed. The test never grants actual
service-control permissions or operates on a Riesel app. A root fixture checker
supplies trusted operation details, and a pty feeder stands in for a person
entering the *generated* test password. Not a human-consent security proof.

Do NOT treat a successful password challenge in a session terminal/GUI as a
trusted input path against compromised same-UID code or X11 input injection.
"""
from __future__ import annotations

import os
from pathlib import Path
import pty
import pwd
import re
import secrets
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid
from xml.sax.saxutils import escape

ACTION_PREFIX = "org.riesel.research.auth.m"
UNIT_PREFIX = "riesel-m59-interactive-"
USER_PREFIX = "rieselm"


def invoke(argv, *, input=None, timeout=12):
    return subprocess.run(
        [str(x) for x in argv], input=input,
        text=True, capture_output=True, timeout=timeout, cwd="/tmp",
        check=False,
    )


def root(*argv, input=None, timeout=12):
    return invoke(["sudo", "-n", *argv], input=input, timeout=timeout)


def proc_start(pid):
    raw = Path(f"/proc/{pid}/stat").read_text()
    return int(raw[raw.rfind(")") + 2:].split()[19])


def require_ok(result, label):
    if result.returncode:
        raise AssertionError(
            f"{label} failed rc={result.returncode}: {result.stderr[-800:]!r}"
        )


def subject_mode():
    # A plain unprivileged process with no effective capabilities; rooted
    # polkit checks must derive pid/start/uid from kernel observations.
    if os.geteuid() == 0:
        raise AssertionError("NSS subject must not run as root")
    status = Path("/proc/self/status").read_text()
    cap = next(x for x in status.splitlines() if x.startswith("CapEff:"))
    if int(cap.split(":", 1)[1], 16):
        raise AssertionError("subject retained effective capabilities")
    print(f"READY {os.getpid()} {proc_start(os.getpid())} {os.getuid()}",
          flush=True)
    sys.stdin.readline()


def subject_spawn(script, uid, gid):
    args = [
        "sudo", "-n", "setpriv", "--reuid", str(uid), "--regid", str(gid),
        "--clear-groups", "/usr/bin/python3", "-u", str(script), "--subject",
    ]
    p = subprocess.Popen(
        args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, cwd="/tmp",
    )
    try:
        import selectors
        sel = selectors.DefaultSelector()
        assert p.stdout is not None
        sel.register(p.stdout, selectors.EVENT_READ)
        try:
            if not sel.select(timeout=8):
                raise AssertionError("unprivileged polkit subject failed to start")
            ready = p.stdout.readline().strip().split()
        finally:
            sel.close()
        if len(ready) != 4 or ready[0] != "READY":
            raise AssertionError(f"invalid subject data: {ready!r}")
        pid, tick, observed_uid = [int(x) for x in ready[1:]]
        # sudo launches a child before dropping to the generated test UID;
        # the actual subject PID is NOT necessarily the sudo wrapper PID.
        # Validate the exact subject PID/start-time/UID against procfs.
        if pid <= 1 or observed_uid != uid or proc_start(pid) != tick:
            raise AssertionError("kernel identity mismatched actual subject")
        if p.poll() is not None:
            raise AssertionError("subject wrapper exited before authorization")
        if int(next(x for x in Path(f"/proc/{pid}/status").read_text().splitlines()
                    if x.startswith("Uid:")).split()[1]) != uid:
            raise AssertionError("kernel subject UID differs")
        return p, f"{pid},{tick},{uid}"
    except Exception:
        p.kill()
        p.wait(timeout=3)
        raise


def check(action, identity, unit, verb, *, interaction=False, agent=False):
    args = ["pkcheck", "--action-id", action, "--process", identity,
            "--detail", "unit", unit, "--detail", "verb", verb]
    if interaction:
        args.append("--allow-user-interaction")
    if agent:
        args.append("--enable-internal-agent")
    return root(*args)


def authenticate_with_text_agent(action, identity, unit, password):
    # A disposable PTY supplies the generated test password to the real
    # polkit textual authentication agent. This is an automated credential
    # exercise, NOT a trusted human/operator approval on a desktop.
    argv = ["sudo", "-n", "pkcheck", "--action-id", action,
            "--process", identity, "--detail", "unit", unit,
            "--detail", "verb", "stop",
            "--allow-user-interaction", "--enable-internal-agent"]
    child, fd = pty.fork()
    if child == 0:
        try:
            os.execvp(argv[0], argv)
        finally:
            os._exit(127)
    transcript = bytearray()
    supplied = False
    deadline = time.monotonic() + 24
    status = None
    try:
        while time.monotonic() < deadline:
            got, state = os.waitpid(child, os.WNOHANG)
            if got == child:
                status = state
                break
            ready, _, _ = select.select([fd], [], [], 0.25)
            if ready:
                try:
                    packet = os.read(fd, 4096)
                except OSError:
                    packet = b""
                if packet:
                    transcript.extend(packet)
                    if len(transcript) > 12000:
                        raise AssertionError("auth prompt unexpectedly verbose")
                    # This string is not a trusted prompt assertion: the
                    # polkit fixture runs a real pkcheck and generated policy.
                    if not supplied and b"password" in bytes(transcript).lower():
                        os.write(fd, (password + "\n").encode())
                        supplied = True
            else:
                continue
        if status is None:
            got, state = os.waitpid(child, os.WNOHANG)
            if got == child:
                status = state
        if status is None:
            raise AssertionError(
                "polkit textual authentication did not terminate before timeout"
            )
        code = os.waitstatus_to_exitcode(status)
        if code or not supplied:
            clean = re.sub(rb"(?i)password[^:\r\n]*:", b"PASSWORD_PROMPT:",
                           bytes(transcript))
            # Do not write generated password into public CI logs.
            summary = clean.decode(errors="replace").replace(password, "[REDACTED]")
            raise AssertionError(
                f"polkit challenge not completed: exit={code}, "
                f"password_prompt={supplied}, transcript_tail={summary[-900:]!r}"
            )
        print("RESEARCH_M59_POLKIT_REAL_PAM_CREDENTIAL_AUTHENTICATED", flush=True)
    finally:
        if status is None:
            try:
                os.kill(child, signal.SIGKILL)
            except ProcessLookupError:
                pass
            try:
                os.waitpid(child, 0)
            except ChildProcessError:
                pass
        os.close(fd)



def authenticate_with_x11_submit(action, identity, unit, password, nonce):
    """Research-only: X11 outsider completes a real polkit textual challenge.

    The test driver enters the disposable secret but withholds Return. A
    separate unprivileged X11 client receives only the xterm window and PID,
    then submits Return. This proves an input-origin boundary, NOT malicious
    authorization of a different operation or successful secret theft.
    """
    if not os.environ.get("DISPLAY"):
        raise AssertionError("X11 DISPLAY missing")
    title = f"riesel-polkit-research-{nonce}"
    cmd = [
        "xterm", "-T", title, "-geometry", "100x26+0+0",
        "-e", "sudo", "-n", "pkcheck",
        "--action-id", action, "--process", identity,
        "--detail", "unit", unit, "--detail", "verb", "stop",
        "--allow-user-interaction", "--enable-internal-agent",
    ]
    terminal = subprocess.Popen(
        cmd, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, cwd="/tmp",
    )
    try:
        window = None
        for _ in range(100):
            if terminal.poll() is not None:
                raise AssertionError(
                    "X11 polkit terminal exited before test input: "
                    + terminal.stderr.read()[-650:]
                )
            found = invoke(["xdotool", "search", "--onlyvisible", "--name",
                            title], timeout=5)
            ids = [line.strip() for line in found.stdout.splitlines()
                   if line.strip().isdecimal()]
            if found.returncode == 0 and len(ids) == 1:
                window = ids[0]
                break
            time.sleep(0.1)
        if window is None:
            raise AssertionError("bounded X11 polkit window search failed")
        actual_title = invoke(["xdotool", "getwindowname", window])
        if actual_title.returncode or actual_title.stdout.strip() != title:
            raise AssertionError("X11 target not exact disposable auth terminal")
        require_ok(invoke(["xdotool", "windowfocus", "--sync", window]),
                   "focus disposable auth terminal")
        time.sleep(1.0)
        if terminal.poll() is not None:
            raise AssertionError("real polkit prompt not active before input")
        # xdotool --file - reads input from stdin. The password is never an
        # argument to the independent attacker or to xdotool.
        typed = invoke(["xdotool", "type", "--clearmodifiers", "--delay",
                        "30", "--file", "-"], input=password, timeout=10)
        require_ok(typed, "type generated test password without Return")
        time.sleep(0.2)
        if terminal.poll() is not None:
            raise AssertionError("authentication completed without submit")
        print("RESEARCH_M59_X11_TEST_CREDENTIAL_TYPED_NO_CONFIRM", flush=True)
        attacker = Path(__file__).with_name(
            "manager_polkit_x11_submit_attacker.py")
        if not attacker.exists():
            raise AssertionError("independent X11 test client missing")
        result = invoke([sys.executable, str(attacker), window,
                         str(terminal.pid)], timeout=12)
        require_ok(result, "attacker-generated X11 Return")
        if "RESEARCH_M59_X11_INDEPENDENT_SAME_UID_INJECTED_AUTH_SUBMIT" not in result.stdout:
            raise AssertionError("independent X11 attacker marker absent")
        result_code = terminal.wait(timeout=18)
        if result_code:
            raise AssertionError(
                f"real X11 polkit challenge failed rc={result_code}: "
                f"{terminal.stderr.read()[-800:]}"
            )
        print("RESEARCH_M59_REAL_POLKIT_AUTH_COMPLETED_BY_OTHER_X11_CLIENT",
              flush=True)
    finally:
        if terminal.poll() is None:
            terminal.terminate()
        try:
            terminal.wait(timeout=4)
        except subprocess.TimeoutExpired:
            terminal.kill()
            terminal.wait(timeout=4)
        if terminal.stdout is not None:
            terminal.stdout.close()
        if terminal.stderr is not None:
            terminal.stderr.close()


def main(*, keep: bool = False, x11: bool = False):
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("Linux unprivileged disposable test host required")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("actual systemd and polkit required")
    if keep and x11:
        raise AssertionError("KEEP and X11 modes must be separate")
    for command in ("pkcheck", "pkaction", "setpriv", "sudo"):
        if shutil.which(command) is None:
            raise AssertionError(f"missing necessary OS tool {command}")
    if x11 and (not os.environ.get("DISPLAY") or
                not shutil.which("xdotool") or not shutil.which("xterm")):
        raise AssertionError("real X11 fixture tools missing")
    require_ok(root("true"), "test root fixture prerequisite")
    nonce = uuid.uuid4().hex[:12]
    action = ACTION_PREFIX + nonce
    unit = UNIT_PREFIX + nonce + ".service"
    user = USER_PREFIX + nonce[:8]
    if not re.fullmatch(r"org\.riesel\.research\.auth\.m[a-f0-9]{12}", action):
        raise AssertionError("invalid action identifier")
    if not re.fullmatch(r"riesel-m59-interactive-[a-f0-9]{12}\.service", unit):
        raise AssertionError("invalid target label")
    if not re.fullmatch(r"rieselm[a-f0-9]{8}", user):
        raise AssertionError("invalid temporary NSS account")
    try:
        pwd.getpwnam(user)
    except KeyError:
        pass
    else:
        raise AssertionError("generated test user already exists")
    password = secrets.token_hex(12) if x11 else secrets.token_urlsafe(22)
    fixture = Path(tempfile.mkdtemp(prefix="riesel-polkit-interactive-"))
    fixture.chmod(0o755)
    script_copy = fixture / "subject.py"
    shutil.copyfile(Path(__file__).resolve(), script_copy)
    script_copy.chmod(0o644)
    action_dest = Path(f"/usr/share/polkit-1/actions/{action}.policy")
    rule_dest = Path(f"/etc/polkit-1/rules.d/00-riesel-interactive-{nonce}.rules")
    policy = f'''<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
  "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <action id="{escape(action)}">
    <description>Disposable Riesel authentication research</description>
    <message>Authenticate to inspect disposable Riesel operation</message>
    <defaults>
      <allow_any>no</allow_any><allow_inactive>no</allow_inactive>
      <allow_active>no</allow_active>
    </defaults>
  </action>
</policyconfig>
'''
    authorization_mode = "AUTH_SELF_KEEP" if keep else "AUTH_SELF"
    rules = (f'polkit.addRule(function(action, subject) {{\n'
             f'  if (action.id === "{action}" && subject.user === "{user}" &&\n'
             f'      action.lookup("unit") === "{unit}" &&\n'
             f'      action.lookup("verb") === "stop") {{\n'
             f'    return polkit.Result.{authorization_mode};\n'
             f'  }}\n'
             f'}});\n')
    (fixture / "action.policy").write_text(policy)
    (fixture / "action.rules").write_text(rules)
    installed, account = [], False
    clients = []
    try:
        # Never install an authorization for real systemctl or Manager action.
        require_ok(root("useradd", "--no-create-home", "--user-group",
                        "--shell", "/bin/bash", user), "create test NSS account")
        account = True
        require_ok(root("chpasswd", input=f"{user}:{password}\n"),
                   "set disposable random test password")
        test_uid = pwd.getpwnam(user).pw_uid
        test_gid = pwd.getpwnam(user).pw_gid
        if test_uid in (0, os.geteuid()):
            raise AssertionError("not a separate unprivileged NSS subject")
        for src, dest in ((fixture / "action.policy", action_dest),
                          (fixture / "action.rules", rule_dest)):
            if root("test", "-e", str(dest)).returncode == 0:
                raise AssertionError("refusing to replace existing polkit policy")
            require_ok(root("install", "-m", "0644", str(src), str(dest)),
                       "install uniquely named disposable polkit test action")
            installed.append(dest)
        for _ in range(30):
            if invoke(["pkaction", "--action-id", action]).returncode == 0:
                break
            time.sleep(0.2)
        else:
            raise AssertionError("real polkitd did not register test action")

        process_a, identity_a = subject_spawn(script_copy, test_uid, test_gid)
        clients.append(process_a)
        process_b, identity_b = subject_spawn(script_copy, test_uid, test_gid)
        clients.append(process_b)
        if identity_a == identity_b:
            raise AssertionError("two independent test process identities required")

        # Neither caller is independently authorized merely for its UID.
        for role, subject_id in (("A", identity_a), ("B", identity_b)):
            response = check(action, subject_id, unit, "stop")
            if response.returncode != 2:
                raise AssertionError(
                    f"expected unfulfilled polkit challenge for {role}: "
                    f"rc={response.returncode}, err={response.stderr[-500:]!r}"
                )
        print("RESEARCH_M59_TWO_SAME_UID_CLIENTS_REQUIRE_REAL_AUTH_CHALLENGE",
              flush=True)
        for target, operation in ((unit + "-wrong", "stop"),
                                  (unit, "restart")):
            result = check(action, identity_a, target, operation)
            if result.returncode != 1:
                raise AssertionError(
                    f"wrong trusted operation should be denied, got rc={result.returncode}"
                )
        print("RESEARCH_M59_POLKIT_WRONG_TRUSTED_DETAILS_DENIED", flush=True)

        # Satisfy one real AUTH_SELF challenge with a randomly generated
        # disposable PAM password. The PTY feeder is *not human consent*.
        if x11:
            authenticate_with_x11_submit(action, identity_a, unit, password,
                                         nonce)
        else:
            authenticate_with_text_agent(action, identity_a, unit, password)
        if keep:
            # This is an intentional NEGATIVE SECURITY CONTROL. polkit's
            # documented _KEEP result permits another check for the same
            # action/subject even if the trusted action details are changed.
            # There is no real protected Manager OS operation in this probe.
            for target, verb in ((unit + "-wrong", "stop"),
                                 (unit, "restart")):
                response = check(action, identity_a, target, verb)
                if response.returncode != 0:
                    raise AssertionError(
                        "expected to reproduce polkit cached detail-substitution: "
                        f"target={target!r} verb={verb!r} "
                        f"rc={response.returncode}, err={response.stderr[-400:]!r}"
                    )
            print("RESEARCH_M59_POLKIT_KEEP_CACHED_AUTH_BYPASSES_CHANGED_DETAILS",
                  flush=True)
            # Do not assert that same-user peer B is distinct from A based
            # solely on the numeric user identity. Report actual authority.
            peer = check(action, identity_b, unit, "stop")
            if peer.returncode not in (0, 1, 2):
                raise AssertionError(
                    f"unexpected second-client polkit response: {peer.returncode}"
                )
            print(f"RESEARCH_M59_POLKIT_KEEP_SECOND_SAME_UID_RC={peer.returncode}",
                  flush=True)
            print("RESEARCH_M59_POLKIT_KEEP_DETAIL_SUBSTITUTION_NEGATIVE_PASS",
                  flush=True)
        else:
            # AUTH_SELF (without _KEEP) must not cache unscoped consent.
            for role, subject_id in (("A_again", identity_a),
                                     ("B_after_A", identity_b)):
                response = check(action, subject_id, unit, "stop")
                if response.returncode != 2:
                    raise AssertionError(
                        f"no-KEEP auth improperly reused by {role}: "
                        f"rc={response.returncode}, err={response.stderr[-400:]!r}"
                    )
            # A fresh-prompt policy must also continue rejecting changed
            # trusted action details AFTER the single successful challenge;
            # do not infer this solely from the original exact target.
            for target, verb in ((unit + "-wrong", "stop"),
                                 (unit, "restart")):
                denied = check(action, identity_a, target, verb)
                if denied.returncode != 1:
                    raise AssertionError(
                        "noncached policy admitted changed action details: "
                        f"unit={target!r}, verb={verb!r}, rc={denied.returncode}"
                    )
            print("RESEARCH_M59_POLKIT_NO_KEEP_CHANGED_DETAILS_STILL_DENIED",
                  flush=True)
            print("RESEARCH_M59_POLKIT_NO_KEEP_SECOND_SAME_UID_REQUIRES_AUTH",
                  flush=True)
            print("RESEARCH_M59_REAL_POLKIT_AUTH_CHALLENGE_DIFFERENTIAL_PASS",
                  flush=True)
    finally:
        for p in clients:
            try:
                if p.stdin is not None:
                    p.stdin.close()
                p.wait(timeout=4)
            except (subprocess.TimeoutExpired, OSError):
                p.kill()
                p.wait(timeout=4)
            if p.stdout is not None:
                p.stdout.close()
            if p.stderr is not None:
                p.stderr.close()
        cleanup_errors = []
        for dest in reversed(installed):
            result = root("rm", "-f", "--", str(dest))
            if result.returncode or root("test", "!", "-e", str(dest)).returncode:
                cleanup_errors.append(f"policy removal {dest.name}")
        if account:
            # A sudo monitor may outlive its direct launcher on fixture
            # failures. The generated account did not exist before this
            # experiment; kill only that disposable account's remaining
            # processes, then remove it. Never address a real user.
            if not re.fullmatch(r"rieselm[a-f0-9]{8}", user):
                raise AssertionError("unsafe disposable-account cleanup name")
            root("pkill", "-KILL", "-u", user)
            for _ in range(30):
                result = root("userdel", user)
                if result.returncode == 0:
                    break
                time.sleep(0.1)
            else:
                cleanup_errors.append(
                    f"account removal {user}: {result.stderr[-400:]}"
                )
        shutil.rmtree(fixture)
        if cleanup_errors:
            raise AssertionError("; ".join(cleanup_errors))
        print("RESEARCH_M59_DISPOSABLE_POLKIT_USER_AND_POLICY_REMOVED", flush=True)


if __name__ == "__main__":
    if len(sys.argv) == 1:
        main()
    elif len(sys.argv) == 2 and sys.argv[1] == "--keep":
        main(keep=True)
    elif len(sys.argv) == 2 and sys.argv[1] == "--x11":
        main(x11=True)
    elif len(sys.argv) == 2 and sys.argv[1] == "--subject":
        subject_mode()
    else:
        raise SystemExit("invalid research entrypoint")
