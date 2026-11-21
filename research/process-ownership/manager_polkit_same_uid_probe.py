#!/usr/bin/env python3
"""#59 M-S2 disposable REAL polkit policy principal/target differential.

This is deliberately a negative authorization experiment, not an auth design.
A temporary action and precisely scoped policy are installed on a disposable
GitHub Actions Linux runner, then uninstalled by an unconditional finally.
No systemd service is started, stopped or granted through the policy.
"""
from __future__ import annotations

import os
from pathlib import Path
import pwd
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid
from xml.sax.saxutils import escape

OTHER_UID = 60090
PREFIX = "org.riesel.research.manager.m"
UNIT_PREFIX = "riesel-research-59-polkit-"


def run(argv: list[str], *, timeout: int = 12) -> subprocess.CompletedProcess[str]:
    return subprocess.run(argv, input="", capture_output=True, text=True,
                          timeout=timeout, check=False, cwd="/tmp")


def sudo(*argv: str) -> subprocess.CompletedProcess[str]:
    return run(["sudo", "-n", *argv])


def start_time() -> int:
    # Linux /proc stat field 22, indexed after the parenthesized comm field.
    raw = Path(f"/proc/{os.getpid()}/stat").read_text()
    return int(raw[raw.rfind(")") + 2:].split()[19])


def subject(action: str, unit: str) -> None:
    # This is an unprivileged client. A rule's action details cannot be
    # supplied directly by an untrusted polkit caller; we verified the
    # PolicyKit daemon returns NotAuthorized (not a normal policy NO).
    if os.geteuid() == 0:
        raise AssertionError("research subject unexpectedly privileged")
    with open("/proc/self/status", encoding="utf-8") as f:
        caps = next(line for line in f if line.startswith("CapEff:"))
    if int(caps.split(":", 1)[1], 16):
        raise AssertionError("subject has effective Linux capabilities")
    identity = f"{os.getpid()},{start_time()},{os.getuid()}"
    direct = run(["pkcheck", "--action-id", action, "--process", identity,
                  "--detail", "unit", unit, "--detail", "verb", "stop"])
    if direct.returncode != 127 or "NotAuthorized" not in direct.stderr:
        raise AssertionError(
            f"expected untrusted detail rejection; rc={direct.returncode}, "
            f"stderr={direct.stderr!r}"
        )
    print("RESEARCH_M59_POLKIT_UNTRUSTED_DIRECT_DETAILS_REJECTED", flush=True)
    print(f"READY {os.getpid()} {start_time()} {os.getuid()}", flush=True)
    sys.stdin.readline()  # remain alive for protected, trusted root check


def verified_subject(process: subprocess.Popen[str], timeout: float = 8.0) -> str:
    import select
    assert process.stdout is not None
    ready, _, _ = select.select([process.stdout], [], [], timeout)
    if not ready:
        raise AssertionError("unprivileged polkit subject did not report status")
    marker = process.stdout.readline().strip()
    if marker != "RESEARCH_M59_POLKIT_UNTRUSTED_DIRECT_DETAILS_REJECTED":
        raise AssertionError(f"unexpected untrusted detail status: {marker!r}")
    # Python's text-mode readline may have buffered the next line.
    identity = process.stdout.readline().strip().split()
    if len(identity) != 4 or identity[0] != "READY":
        raise AssertionError(f"invalid polkit subject identity: {identity!r}")
    pid, tick, uid = map(int, identity[1:])
    if pid <= 1 or pid != process.pid and process.args[0] != "sudo":
        raise AssertionError("unexpected process pid")
    if uid == 0:
        raise AssertionError("polkit subject retained root")
    # The running fixture keeps this /proc identity available. Both
    # observed UID and start-time come from the kernel-owned procfs.
    stat = Path(f"/proc/{pid}/stat").read_text()
    observed_tick = int(stat[stat.rfind(")") + 2:].split()[19])
    if tick != observed_tick:
        raise AssertionError("subject PID/start-time changed")
    return f"{pid},{tick},{uid}"


def trusted_check(action: str, identity: str, unit: str, verb: str,
                  expected: int, role: str) -> None:
    # Only a trusted mechanism (root in this research) can supply
    # the action's unit/verb details to polkit. A real Manager must
    # derive them from the authoritative registration, not client JSON.
    result = run(["sudo", "-n", "pkcheck", "--action-id", action,
                  "--process", identity, "--detail", "unit", unit,
                  "--detail", "verb", verb])
    if expected == 0 and result.returncode != 0:
        raise AssertionError(
            f"{role} expected authorized: rc={result.returncode}; "
            f"stderr={result.stderr}"
        )
    if expected == 1 and result.returncode == 0:
        raise AssertionError(f"{role} unexpectedly authorized")
    if result.returncode in (126, 127):
        raise AssertionError(
            f"{role} invalid/rejected polkit check, not policy evidence: "
            f"{result.stderr}"
        )
    print(f"RESEARCH_M59_POLKIT_{role}_RC_{result.returncode}", flush=True)


def main() -> None:
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("Linux unprivileged disposable CI host required")
    for exe in ("pkcheck", "sudo", "setpriv"):
        if not shutil.which(exe):
            raise AssertionError(f"missing {exe}; runner setup is not evidence")
    check_sudo = sudo("true")
    if check_sudo.returncode:
        raise AssertionError("disposable fixture installer requires sudo -n")

    nonce = uuid.uuid4().hex[:12]
    action = PREFIX + nonce
    unit = f"{UNIT_PREFIX}{nonce}.service"  # only data; never an OS service
    if not re.fullmatch(r"org\.riesel\.research\.manager\.m[a-f0-9]{12}", action):
        raise AssertionError("unsafe research action")
    if not re.fullmatch(r"riesel-research-59-polkit-[a-f0-9]{12}\.service", unit):
        raise AssertionError("unsafe research unit label")

    user = pwd.getpwuid(os.geteuid()).pw_name
    if not re.fullmatch(r"[A-Za-z_][A-Za-z0-9_.-]*", user):
        raise AssertionError("unexpected operator name")
    if os.geteuid() == OTHER_UID:
        raise AssertionError("other principal collides with CI user")

    root = Path(tempfile.mkdtemp(prefix="riesel-m59-polkit-"))
    root.chmod(0o755)
    code = root / "probe.py"
    shutil.copyfile(Path(__file__).resolve(), code)
    code.chmod(0o644)
    action_path = Path(f"/usr/share/polkit-1/actions/{action}.policy")
    rule_path = Path(f"/etc/polkit-1/rules.d/00-riesel-m59-{nonce}.rules")
    # Restrict all OS rule side effects to ONE newly created test action ID.
    # This fixture NEVER authorizes org.freedesktop.systemd1.manage-units.
    policy = f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
<policyconfig>
  <action id="{escape(action)}">
    <description>Temporary Riesel same-UID authorization research</description>
    <message>Authorization test only, no privileged Manager operations</message>
    <defaults>
      <allow_any>no</allow_any>
      <allow_inactive>no</allow_inactive>
      <allow_active>no</allow_active>
    </defaults>
  </action>
</policyconfig>
"""
    rules = (f'polkit.addRule(function(action, subject) {{\n'
             f'  if (action.id === "{action}" &&\n'
             f'      subject.user === "{user}" &&\n'
             f'      action.lookup("unit") === "{unit}" &&\n'
             f'      action.lookup("verb") === "stop") {{\n'
             f'    return polkit.Result.YES;\n'
             f'  }}\n'
             f'}});\n')
    local_policy = root / "research.policy"
    local_rule = root / "research.rules"
    local_policy.write_text(policy)
    local_rule.write_text(rules)
    installed = []
    try:
        for local, dest in ((local_policy, action_path), (local_rule, rule_path)):
            # Exact, random, non-existing dest; never clobber a real OS policy.
            exists = sudo("test", "-e", str(dest))
            if exists.returncode == 0:
                raise AssertionError("research policy file already exists")
            result = sudo("install", "-m", "0644", str(local), str(dest))
            if result.returncode:
                raise AssertionError(f"install research policy failed: {result.stderr}")
            installed.append(dest)

        # Observe policy/action visibility on the actual polkit daemon.
        ready = False
        for _ in range(25):
            result = run(["pkaction", "--action-id", action])
            if result.returncode == 0:
                ready = True
                break
            time.sleep(0.2)
        if not ready:
            raise AssertionError("actual polkit action not registered")

        # Real PID/start-time/UID subjects, each a separate non-root process.
        # The root checker receives only kernel-verified subject information,
        # while the trusted operation/target comes from the test's own policy.
        clients = []
        try:
            def spawn(role: str, other: bool = False):
                cmd = [sys.executable, "-u", str(code), "--subject", action, unit]
                if other:
                    cmd = ["sudo", "-n", "setpriv",
                           f"--reuid={OTHER_UID}", f"--regid={OTHER_UID}",
                           "--clear-groups", "/usr/bin/python3", "-u",
                           str(code), "--subject", action, unit]
                p = subprocess.Popen(cmd, stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE,
                                     stderr=subprocess.PIPE, text=True,
                                     cwd="/tmp")
                clients.append(p)
                return verified_subject(p)

            legit = spawn("OPERATOR")
            hostile = spawn("HOSTILE_SAME_UID")
            foreign = spawn("FOREIGN_UID", other=True)
            if legit == hostile:
                raise AssertionError("subject identities must differ")
            if legit.split(",")[2] != hostile.split(",")[2]:
                raise AssertionError("same-UID subjects not same user")
            if legit.split(",")[2] == foreign.split(",")[2]:
                raise AssertionError("foreign-UID subject not distinct")

            trusted_check(action, legit, unit, "stop", 0, "OPERATOR")
            trusted_check(action, hostile, unit, "stop", 0,
                          "HOSTILE_SAME_UID")
            print("RESEARCH_M59_POLKIT_SAME_UID_CLIENTS_BOTH_AUTHORIZED",
                  flush=True)

            trusted_check(action, hostile, unit + ".other", "stop", 1,
                          "WRONG_UNIT")
            trusted_check(action, hostile, unit, "restart", 1,
                          "WRONG_VERB")
            trusted_check(action, foreign, unit, "stop", 1,
                          "FOREIGN_UID")
            print("RESEARCH_M59_POLKIT_ACTION_DETAILS_AND_OTHER_UID_DENIED",
                  flush=True)
            print("RESEARCH_M59_POLKIT_USER_POLICY_NO_CLIENT_ATTESTATION_PASS",
                  flush=True)
        finally:
            for p in clients:
                try:
                    if p.stdin is not None:
                        p.stdin.close()
                    p.wait(timeout=5)
                except (subprocess.TimeoutExpired, OSError):
                    p.kill()
                    p.wait(timeout=5)
                if p.stdout is not None:
                    p.stdout.close()
                if p.stderr is not None:
                    p.stderr.close()
    finally:
        # Remove exact random files only; never wildcard policy actions or
        # change systemd units, admins, sudoers, or other polkit rules.
        for p in reversed(installed):
            sudo("rm", "-f", "--", str(p))
        shutil.rmtree(root)


if __name__ == "__main__":
    if len(sys.argv) == 1:
        main()
    elif len(sys.argv) == 4 and sys.argv[1] == "--subject":
        subject(sys.argv[2], sys.argv[3])
    else:
        raise SystemExit("invalid research mode")
