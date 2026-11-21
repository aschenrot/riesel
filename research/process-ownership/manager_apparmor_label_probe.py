#!/usr/bin/env python3
"""#59 M-S2 actual AppArmor peer-label differential; Linux research ONLY.

Loads two uniquely named, deliberately non-production AppArmor profiles:
a permissive allowlisted client profile and a separate confined attacker
profile with no profile-switch rights. Compares real same-UID SO_PEERSEC,
unconfined self-entry, and restricted process profile-switch denial. No systemd/Manager
service, protected application or real privileged action is ever changed.
"""
from __future__ import annotations
import os
from pathlib import Path
import re
import select
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import uuid

ATTACK_UID = 60090
BASE = "/tmp/riesel-m59-apparmor-"


def invoke(args, *, timeout=14):
    return subprocess.run(args, capture_output=True, text=True,
                          stdin=subprocess.DEVNULL, cwd="/tmp", timeout=timeout,
                          check=False)


def checked(args, timeout=14):
    r = invoke(args, timeout=timeout)
    if r.returncode != 0:
        raise AssertionError(
            f"failed {args[:3]!r}, rc={r.returncode}, "
            f"stdout={r.stdout[:400]!r}, stderr={r.stderr[:1100]!r}"
        )
    return r


def sudo(*args):
    return ["sudo", "-n", *args]


def server(path, profile):
    if os.geteuid() != 0:
        raise AssertionError("probe verifier must be root")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as listener:
        listener.settimeout(8)
        listener.bind(path)
        os.chmod(path, 0o666)
        listener.listen(1)
        print("READY", flush=True)
        with listener.accept()[0] as peer:
            peer.settimeout(6)
            pid, uid, gid = struct.unpack(
                "3i", peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
            )
            if pid <= 1 or uid != ATTACK_UID or gid != ATTACK_UID:
                raise AssertionError(f"unexpected test peer {pid}:{uid}:{gid}")
            label = peer.getsockopt(
                socket.SOL_SOCKET, socket.SO_PEERSEC, 512
            ).rstrip(b"\x00").decode("utf-8", errors="replace")
            request = peer.recv(32)
            is_profile = label == profile or label.startswith(profile + " ")
            allowed = is_profile and request == b"STOP\n"
            peer.sendall(b"ALLOW\n" if allowed else b"DENY\n")
            print(f"RESEARCH_M59_APPARMOR_SOCKET_PEER_PROFILE={is_profile}",
                  flush=True)


def client(path):
    if os.geteuid() != ATTACK_UID or os.getegid() != ATTACK_UID:
        raise AssertionError("client did not drop to same untrusted UID/GID")
    with open("/proc/self/attr/current", encoding="utf-8") as f:
        local = f.read().strip()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(6)
        sock.connect(path)
        sock.sendall(b"STOP\n")
        reply = sock.recv(32).decode().strip()
    print(f"REPLY={reply} LOCAL_LABEL={local}", flush=True)


def runner():
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("needs unprivileged disposable Ubuntu coordinator")
    for name in ("apparmor_parser", "aa-exec", "setpriv"):
        if not shutil.which(name):
            raise AssertionError(f"{name} not installed; no security claim")
    checked(sudo("true"))
    nonce = uuid.uuid4().hex[:12]
    profile = "riesel_m59_" + nonce
    attacker_profile = "riesel_m59_attacker_" + nonce
    root = Path(BASE + nonce)
    if not re.fullmatch(r"/tmp/riesel-m59-apparmor-[a-f0-9]{12}", str(root)):
        raise AssertionError("unsafe research root")
    src_root = Path(tempfile.mkdtemp(prefix="riesel-m59-label-build-"))
    policy_file = src_root / "research.profile"
    # Deliberately permissive to test the *label*, not claim confinement of
    # protected app data, signals, GUI buses or binary loading.
    policy_file.write_text(
        '#include <tunables/global>\n'
        f'profile {profile} flags=(attach_disconnected) {{\n'
        '  #include <abstractions/base>\n'
        '  file,\n'
        '  network,\n'
        '  unix,\n'
        '  capability,\n'
        '  /** ix,\n'
        '}\n'
        # Separate, *conditionally* restricted attacker profile. It allows
        # normal Python/aa-exec execution but does not grant change_profile
        # to the allowlisted domain. This tests only a constrained attacker;
        # an unconfined same-UID process may still self-enter the target.
        f'profile {attacker_profile} flags=(attach_disconnected) {{\n'
        '  #include <abstractions/base>\n'
        '  file,\n'
        '  network,\n'
        '  unix,\n'
        '  capability,\n'
        '  /** ix,\n'
        '}\n'
    )
    created = False
    loaded = False
    try:
        checked(sudo("mkdir", "--", str(root)))
        created = True
        checked(sudo("chmod", "0755", str(root)))
        script = root / "label_probe.py"
        checked(sudo("install", "-o", "root", "-g", "root", "-m", "0644",
                     str(Path(__file__).resolve()), str(script)))
        checked(sudo("apparmor_parser", "-r", "-W", str(policy_file)))
        loaded = True
        print("RESEARCH_M59_APPARMOR_DISPOSABLE_PROFILE_LOADED", flush=True)

        def run_case(client_command, expected, suffix, observed_label=None):
            endpoint = root / f"socket-{suffix}.sock"
            owner = subprocess.Popen(
                sudo("/usr/bin/python3", "-u", str(script),
                     "--server", str(endpoint), profile),
                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                stderr=subprocess.PIPE, text=True, cwd="/tmp"
            )
            assert owner.stdout is not None
            try:
                ready, _, _ = select.select([owner.stdout], [], [], 8)
                if not ready or owner.stdout.readline().strip() != "READY":
                    err = owner.stderr.read() if owner.poll() is not None else ""
                    raise AssertionError(f"verifier not ready: {err[-1200:]}")
                res = invoke(client_command)
                if res.returncode != 0 or f"REPLY={expected}" not in res.stdout:
                    raise AssertionError(
                        f"{suffix} wrong result: rc={res.returncode}, "
                        f"out={res.stdout!r} err={res.stderr[:1100]!r}"
                    )
                if expected == "ALLOW" and profile not in res.stdout:
                    raise AssertionError("kernel security context not inherited")
                if observed_label is not None and observed_label not in res.stdout:
                    raise AssertionError(
                        f"{suffix} not in declared OS profile: {res.stdout!r}"
                    )
                rc = owner.wait(timeout=8)
                tail = owner.stdout.read()
                if rc != 0:
                    err = owner.stderr.read() if owner.stderr else ""
                    raise AssertionError(f"verifier error {suffix}: {err[-1300:]}")
                marker = f"RESEARCH_M59_APPARMOR_SOCKET_PEER_PROFILE={expected == 'ALLOW'}"
                if marker not in tail:
                    raise AssertionError(f"wrong kernel peer label {tail!r}")
            finally:
                if owner.poll() is None:
                    owner.kill()
                    owner.wait(timeout=4)

        direct = sudo("setpriv", f"--reuid={ATTACK_UID}",
                      f"--regid={ATTACK_UID}", "--clear-groups",
                      "/usr/bin/python3", str(script), "--client")
        root_launch = sudo("aa-exec", "-p", profile, "--",
                           "/usr/bin/setpriv", f"--reuid={ATTACK_UID}",
                           f"--regid={ATTACK_UID}", "--clear-groups",
                           "/usr/bin/python3", str(script), "--client")
        run_case([*direct, str(root / "socket-direct.sock")], "DENY", "direct")
        print("RESEARCH_M59_APPARMOR_UNCONFINED_SAME_UID_DENIED", flush=True)
        run_case([*root_launch, str(root / "socket-root.sock")], "ALLOW", "root")
        print("RESEARCH_M59_APPARMOR_ROOT_LAUNCHED_LABEL_ALLOWED", flush=True)

        # This has *no* sudo inside the untrusted user process. Depending on
        # the kernel/AppArmor unprivileged-unconfined restrictions, aa-exec
        # into an installed label may either succeed or fail. Record either
        # outcome, do not assert the intended security conclusion in advance.
        attempt = sudo(
            "setpriv", f"--reuid={ATTACK_UID}", f"--regid={ATTACK_UID}",
            "--clear-groups", "/usr/bin/aa-exec", "-p", profile, "--",
            "/usr/bin/cat", "/proc/self/attr/current"
        )
        entered = invoke(attempt)
        if entered.returncode == 0:
            if profile not in entered.stdout:
                raise AssertionError(
                    f"aa-exec exited successfully without requested label: "
                    f"{entered.stdout!r}"
                )
            self_launch = sudo(
                "setpriv", f"--reuid={ATTACK_UID}", f"--regid={ATTACK_UID}",
                "--clear-groups", "/usr/bin/aa-exec", "-p", profile, "--",
                "/usr/bin/python3", str(script), "--client",
                str(root / "socket-self.sock")
            )
            run_case(self_launch, "ALLOW", "self")
            print("RESEARCH_M59_APPARMOR_UNPRIVILEGED_SELF_LABEL_ALLOWED",
                  flush=True)
        else:
            print("RESEARCH_M59_APPARMOR_UNPRIVILEGED_SELF_LABEL_DENIED",
                  flush=True)
        # Explicitly enforce a *conditional* security-domain boundary:
        # the attacker is first placed by the root fixture into a separate
        # confined domain without any transition permission to the trusted
        # client domain. This illustrates why security-label *entry*
        # policy matters, while keeping the unconfined bypass above visible.
        restricted = sudo(
            "aa-exec", "-p", attacker_profile, "--",
            "/usr/bin/setpriv", f"--reuid={ATTACK_UID}",
            f"--regid={ATTACK_UID}", "--clear-groups",
            "/usr/bin/python3", str(script), "--client"
        )
        run_case([*restricted, str(root / "socket-restricted.sock")],
                 "DENY", "restricted", observed_label=attacker_profile)
        print("RESEARCH_M59_APPARMOR_CONFINED_ATTACKER_SAME_UID_DENIED",
              flush=True)

        attempted_change = sudo(
            "aa-exec", "-p", attacker_profile, "--",
            "/usr/bin/setpriv", f"--reuid={ATTACK_UID}",
            f"--regid={ATTACK_UID}", "--clear-groups",
            "/usr/bin/aa-exec", "-p", profile, "--",
            "/usr/bin/cat", "/proc/self/attr/current"
        )
        blocked = invoke(attempted_change)
        if blocked.returncode == 0:
            raise AssertionError(
                "attacker confined without transition rights nevertheless "
                f"entered trusted profile: stdout={blocked.stdout!r}"
            )
        stderr = blocked.stderr.lower()
        # A missing executable, path, account, parser, or profile is a
        # fixture error; an actual AppArmor security policy rejection is
        # evidence. Require a concrete permission-denied error, not merely
        # a nonzero return code from aa-exec.
        if "permission denied" not in stderr and "operation not permitted" not in stderr:
            raise AssertionError(
                f"restricted transition failed without permission denial: "
                f"rc={blocked.returncode}, stderr={blocked.stderr[:1100]!r}"
            )
        if profile in blocked.stdout and attacker_profile not in blocked.stdout:
            raise AssertionError("rejected attempt unexpectedly used trusted label")
        print("RESEARCH_M59_APPARMOR_CONFINED_SWITCH_PERMISSION_DENIED",
              flush=True)
        print("RESEARCH_M59_APPARMOR_CONFINED_ATTACKER_PROFILE_SWITCH_DENIED",
              flush=True)
        print("RESEARCH_M59_APPARMOR_CONDITIONAL_RESTRICTED_DOMAIN_PASS",
              flush=True)
        print("RESEARCH_M59_APPARMOR_SO_PEERSEC_LABEL_DIFFERENTIAL_PASS",
              flush=True)
    finally:
        if loaded:
            unload = invoke(sudo("apparmor_parser", "-R", str(policy_file)))
            if unload.returncode:
                raise AssertionError(
                    f"temporary profile unload failed: {unload.stderr}"
                )
            print("RESEARCH_M59_APPARMOR_DISPOSABLE_PROFILE_UNLOADED", flush=True)
        if created:
            removed = invoke(sudo("rm", "-rf", "--", str(root)))
            if removed.returncode or root.exists():
                raise AssertionError("temporary trusted fixture still exists")
        shutil.rmtree(src_root)


if __name__ == "__main__":
    if len(sys.argv) == 1:
        runner()
    elif len(sys.argv) == 4 and sys.argv[1] == "--server":
        server(sys.argv[2], sys.argv[3])
    elif len(sys.argv) == 3 and sys.argv[1] == "--client":
        client(sys.argv[2])
    else:
        raise SystemExit("invalid AppArmor probe mode")
