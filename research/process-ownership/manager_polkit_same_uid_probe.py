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


def check(action: str, unit: str, verb: str) -> subprocess.CompletedProcess[str]:
    identity = f"{os.getpid()},{start_time()},{os.getuid()}"
    return run(["pkcheck", "--action-id", action, "--process", identity,
                "--detail", "unit", unit, "--detail", "verb", verb])


def client(action: str, unit: str, verb: str, role: str, expected: int) -> None:
    if os.geteuid() == 0:
        raise AssertionError("polkit test client unexpectedly root")
    with open("/proc/self/status", encoding="utf-8") as f:
        caps = next(line for line in f if line.startswith("CapEff:"))
    if int(caps.split(":", 1)[1], 16):
        raise AssertionError("client has effective Linux capabilities")
    result = check(action, unit, verb)
    if expected == 0 and result.returncode != 0:
        raise AssertionError(
            f"{role} expected authorized: rc={result.returncode}; err={result.stderr}"
        )
    if expected == 1 and result.returncode == 0:
        raise AssertionError(f"{role} unexpectedly authorized")
    if result.returncode == 127:
        raise AssertionError(f"polkit client error, not a security result: {result.stderr}")
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

        def invoke(role: str, verb: str, target: str, expected: int, other: bool = False):
            args = [sys.executable, str(code), "--client", action, target, verb,
                    role, str(expected)]
            if other:
                args = ["sudo", "-n", "setpriv", f"--reuid={OTHER_UID}",
                        f"--regid={OTHER_UID}", "--clear-groups",
                        "/usr/bin/python3", str(code), "--client", action,
                        target, verb, role, str(expected)]
            res = run(args)
            if res.returncode:
                raise AssertionError(f"{role} failed: {res.stderr[:600]!r}, out={res.stdout!r}")
            return res.stdout

        # Observe policy/action visibility on a real polkit daemon.
        ready = False
        for _ in range(25):
            result = run(["pkaction", "--action-id", action])
            if result.returncode == 0:
                ready = True
                break
            time.sleep(0.2)
        if not ready:
            raise AssertionError("actual polkit action was not registered")

        # All roles are separate short-lived process PIDs. The "operator"
        # role is a *test label*, not a trusted OS code identity.
        trusted = invoke("OPERATOR", "stop", unit, 0)
        hostile = invoke("HOSTILE_SAME_UID", "stop", unit, 0)
        for output in (trusted, hostile):
            if "_RC_0" not in output:
                raise AssertionError(f"expected actual polkit success: {output}")
        print("RESEARCH_M59_POLKIT_SAME_UID_CLIENTS_BOTH_AUTHORIZED", flush=True)

        forbidden1 = invoke("WRONG_UNIT", "stop", unit + ".other", 1)
        forbidden2 = invoke("WRONG_VERB", "restart", unit, 1)
        foreign = invoke("FOREIGN_UID", "stop", unit, 1, other=True)
        if "_RC_0" in forbidden1 + forbidden2 + foreign:
            raise AssertionError("polkit wrong-detail or foreign subject was accepted")
        print("RESEARCH_M59_POLKIT_ACTION_DETAILS_AND_OTHER_UID_DENIED", flush=True)
        print("RESEARCH_M59_POLKIT_USER_POLICY_NO_CLIENT_ATTESTATION_PASS", flush=True)
    finally:
        # Remove exact random files only; never wildcard policy actions or
        # change systemd units, admins, sudoers, or other polkit rules.
        for p in reversed(installed):
            sudo("rm", "-f", "--", str(p))
        shutil.rmtree(root)


if __name__ == "__main__":
    if len(sys.argv) == 1:
        main()
    elif len(sys.argv) == 7 and sys.argv[1] == "--client":
        _, _, action, unit, verb, role, expected = sys.argv
        client(action, unit, verb, role, int(expected))
    else:
        raise SystemExit("invalid research mode")
