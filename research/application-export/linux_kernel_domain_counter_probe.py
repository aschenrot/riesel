#!/usr/bin/env python3
"""#60/#75 same-UID AppArmor domain admission versus real Riesel Counter.

A trusted ROOT *test launcher* creates a real AppArmor domain, starts the
unchanged #149 private-network systemd gateway around the unchanged #90
SDK-resolved Counter, and runs two distinct processes under UID60090.

Required negative control: unconfined same-UID users may be able to
self-enter a permissive trusted profile, so a matching label alone is
NOT trusted software, independently authorized human intent or strong
protection without OS-enforced profile entry for ALL adversaries.
"""
from __future__ import annotations

import importlib.util
import json
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

GATEWAY_SHA = "a5c811fbce184e8ff2684ab413798453e6d189f4"
COUNTER_SHA = "5201e61cc74bab778dfc63a3788f1708b2530f21"
ALLOWED_UID = 60090
OUTSIDER_UID = 60091
COUNTER_UID = 60092
HERE = Path(__file__).resolve().parent
WRAPPER = HERE / "linux_kernel_domain_export_server.py"


def cmd(args, *, check=True, timeout=16):
    p = subprocess.run(
        [str(x) for x in args], cwd="/tmp", capture_output=True,
        text=True, stdin=subprocess.DEVNULL, timeout=timeout,
    )
    if check and p.returncode:
        raise AssertionError(
            f"OS test failed {args[:5]!r} rc={p.returncode} "
            f"stdout={p.stdout[-700:]!r} stderr={p.stderr[-1100:]!r}")
    return p


def sudo(*parts):
    return ["sudo", "-n", *parts]


def pinned(path, sha):
    found = cmd(["git", "-C", str(path), "rev-parse", "HEAD"]).stdout.strip()
    if found != sha:
        raise AssertionError(f"must reuse the exact independent tested source {sha}: {found}")


def safe_install(path):
    if not re.fullmatch(r"/opt/riesel-m60-protected-export-[a-f0-9]{12}",
                        str(path)):
        raise AssertionError("refusing non-research installed path")
    return path


def safe_runtime(path):
    if not re.fullmatch(r"/run/riesel-m60-protected-export-[a-f0-9]{12}",
                        str(path)):
        raise AssertionError("refusing non-research runtime path")
    return path


def safe_unit(name):
    if not re.fullmatch(r"riesel-m60-export-[a-f0-9]{12}\.service", name):
        raise AssertionError("refusing non-research systemd unit")
    return name


def import_probe(pr):
    source = pr / "research/application-export/linux_isolated_export_probe.py"
    if not source.is_file():
        raise AssertionError("missing SHA-pinned original OS research")
    spec = importlib.util.spec_from_file_location("riesel_pinned_os_probe", source)
    if spec is None or spec.loader is None:
        raise AssertionError("could not load pinned OS probe")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def client(program, socket, operation, amount, epoch, *, mode, label=None,
           uid=ALLOWED_UID, claimed_uid=None):
    # The command uses sudo only from the trusted CI TEST coordinator.
    # Every actual socket client drops to the ordinary numeric UID first;
    # it cannot invoke sudo or install policy itself.
    body = ["/usr/bin/python3", str(program), "--client", str(socket),
            operation, str(amount), epoch, str(uid if claimed_uid is None
                                               else claimed_uid)]
    drop = ["/usr/bin/setpriv", "--reuid", str(uid), "--regid", str(uid),
            "--clear-groups"]
    if mode == "root-profile":
        if not label:
            raise AssertionError("missing trusted root-selected label")
        args = sudo("/usr/bin/aa-exec", "-p", label, "--", *drop, *body)
    elif mode == "root-attacker-profile":
        if not label:
            raise AssertionError("missing attacker root-selected label")
        args = sudo("/usr/bin/aa-exec", "-p", label, "--", *drop, *body)
    elif mode == "unconfined":
        args = sudo(*drop, *body)
    elif mode == "unconfined-self-enter":
        if not label:
            raise AssertionError("missing attempted attacker-selected label")
        args = sudo(*drop, "/usr/bin/aa-exec", "-p", label, "--", *body)
    else:
        raise AssertionError("unknown test mode")
    result = cmd(args, check=False)
    if result.returncode:
        return None, result
    try:
        return json.loads(result.stdout), result
    except json.JSONDecodeError as err:
        raise AssertionError(
            f"real client returned non-JSON after success: {result.stdout!r}"
        ) from err


def expected_error(result, reason, uid=None, label=None):
    if not isinstance(result, dict) or result.get("error") != reason:
        raise AssertionError(f"expected {reason}, got {result!r}")
    if uid is not None and result.get("observed_uid") != uid:
        raise AssertionError(f"kernel UID not observed: {result!r}")
    if label is not None and not result.get("observed_label", "").startswith(label):
        raise AssertionError(f"kernel label did not match: {result!r}")


def expected_value(result, value):
    if not isinstance(result, dict) or result.get("result") != {"Ok": value}:
        raise AssertionError(
            f"actual Riesel Counter expected {value}, received {result!r}")


def apparmor_profiles(trusted, attacker):
    # Deliberately permissive; investigate *peer label trust*, not full
    # confinement, secret handling, GUI injection or binary provenance.
    def single(name):
        return (
            f"profile {name} flags=(attach_disconnected) {{\n"
            "  #include <abstractions/base>\n"
            "  file,\n"
            "  network,\n"
            "  unix,\n"
            "  capability,\n"
            "  /** ix,\n"
            "}\n"
        )
    return "#include <tunables/global>\n" + single(trusted) + single(attacker)


def assert_host_prerequisites():
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("needs ordinary nonroot Ubuntu CI coordinator")
    if Path("/proc/1/comm").read_text().strip() != "systemd":
        raise AssertionError("must have real systemd as PID1")
    for name in ("apparmor_parser", "aa-exec", "setpriv", "nsenter", "systemd-run"):
        if shutil.which(name) is None:
            raise AssertionError(f"real OS security prerequisite absent: {name}")
    cmd(sudo("true"))
    for uid in (ALLOWED_UID, OUTSIDER_UID, COUNTER_UID):
        try:
            user = pwd.getpwuid(uid)
        except KeyError:
            continue
        raise AssertionError(f"refusing existing system user {uid}: {user.pw_name}")
    if not Path("/sys/module/apparmor/parameters/enabled").exists():
        raise AssertionError("kernel AppArmor enforcement prerequisite absent")
    if Path("/sys/module/apparmor/parameters/enabled").read_text().strip() != "Y":
        raise AssertionError("AppArmor must be enabled, no fake fallback")


def exercise(gateway_root, counter_root):
    assert_host_prerequisites()
    pinned(gateway_root, GATEWAY_SHA)
    pinned(counter_root, COUNTER_SHA)
    original = import_probe(gateway_root)
    binary = counter_root / "target/debug/counter_resolved_facade_host"
    unmodified_server = gateway_root / (
        "research/application-export/linux_isolated_export_server.py"
    )
    for p in (binary, unmodified_server, WRAPPER):
        if not p.is_file():
            raise AssertionError(f"required pinned research file not found {p}")
    nonce = uuid.uuid4().hex[:12]
    unit = safe_unit("riesel-m60-export-" + nonce + ".service")
    install = safe_install(Path("/opt/riesel-m60-protected-export-" + nonce))
    runtime = safe_runtime(Path("/run/riesel-m60-protected-export-" + nonce))
    trusted = "riesel_m60_client_" + nonce
    attacker = "riesel_m60_attacker_" + nonce
    app = install / "counter_resolved_facade_host"
    gateway = install / "linux_isolated_export_server.py"
    bridge = install / "linux_kernel_domain_export_server.py"
    srcdir = Path(tempfile.mkdtemp(prefix="riesel-m60-label-source-"))
    policy = srcdir / "research.apparmor"
    policy.write_text(apparmor_profiles(trusted, attacker))
    created_install = False
    created_runtime = False
    loaded = False
    started = False
    try:
        if install.exists() or runtime.exists():
            raise AssertionError("generated research path collision")
        cmd(sudo("apparmor_parser", "-r", "-W", str(policy)))
        loaded = True
        print("RESEARCH_M60_ROOT_LOADED_TWO_REAL_APPARMOR_PROFILES", flush=True)
        cmd(sudo("install", "-d", "-m", "0755", "-o", "root", "-g",
                 "root", str(install)))
        created_install = True
        cmd(sudo("install", "-d", "-m", "0755", "-o", "root", "-g",
                 "root", str(runtime)))
        created_runtime = True
        for src, dst in ((binary, app), (unmodified_server, gateway),
                         (WRAPPER, bridge)):
            cmd(sudo("install", "-m", "0755", "-o", "root", "-g", "root",
                     str(src), str(dst)))

        # Original Counter and original #149 gateway remain unchanged;
        # adapter adds only the SO_PEERSEC check before the original
        # UID/operation/epoch admission. OS TCP isolation is required.
        started = True
        cmd(sudo(
            "systemd-run", "--system", "--no-block", "--no-ask-password",
            "--unit", unit, "--property=Type=exec",
            "--property=PrivateNetwork=yes", "--property=Restart=no",
            "--property=KillMode=control-group",
            "--property=TimeoutStopSec=7s",
            "--property=WorkingDirectory=/tmp",
            "/usr/bin/python3", str(bridge), "--serve",
            str(install), str(runtime), trusted
        ))
        evidence, state = original.wait_ready(runtime, unit)
        if evidence.get("app_uid") != COUNTER_UID:
            raise AssertionError("unmodified Counter not under isolated UID")
        service_pid = int(state["MainPID"])
        app_pid = int(evidence["app_pid"])
        if (service_pid <= 1 or app_pid <= 1
            or original.ns_inode(service_pid) == original.ns_inode(1)
            or original.ns_inode(app_pid) != original.ns_inode(service_pid)
            or original.uid_of(app_pid) != COUNTER_UID):
            raise AssertionError("genuine Riesel Counter not inside OS private net")
        endpoint = evidence["backend"]
        _, port = endpoint.rsplit(":", 1)
        import socket
        try:
            public_tcp = socket.create_connection(("127.0.0.1", int(port)),
                                                  timeout=0.4)
        except (OSError, TimeoutError):
            pass
        else:
            public_tcp.close()
            raise AssertionError("protected Counter original TCP bypass open")
        print("RESEARCH_M60_REAL_COUNTER_ISOLATED_AND_LABEL_GATEWAY_CALLABLE",
              flush=True)

        public = runtime / "public.sock"
        control = runtime / "owner/control.sock"
        epoch = evidence["epoch"]
        if original.run_admin(gateway, control, {"action": "status"}).get(
            "epoch"
        ) != epoch:
            raise AssertionError("privileged research issuer not available")
        untrusted_admin = cmd(sudo(
            "setpriv", "--reuid", str(ALLOWED_UID), "--regid",
            str(ALLOWED_UID), "--clear-groups", "/usr/bin/python3",
            str(gateway), "--admin", str(control),
            json.dumps({"action": "grant", "uid": ALLOWED_UID,
                        "operations": ["add"]})
        ), check=False)
        if untrusted_admin.returncode == 0:
            raise AssertionError("same-UID untrusted process reached root issuer")
        original.run_admin(gateway, control, {
            "action": "grant", "uid": ALLOWED_UID,
            "operations": ["value", "add"],
        })

        # All processes under the same UID. Only the privileged TEST
        # launcher assigns the intended trusted process profile.
        unconfined, _ = client(gateway, public, "add", 1, epoch,
                              mode="unconfined")
        expected_error(unconfined, "untrusted_kernel_client_domain",
                       uid=ALLOWED_UID)
        trusted_read, _ = client(gateway, public, "value", 0, epoch,
                                 mode="root-profile", label=trusted)
        expected_value(trusted_read, 0)
        trusted_add, _ = client(gateway, public, "add", 2, epoch,
                                mode="root-profile", label=trusted)
        expected_value(trusted_add, 2)
        print("RESEARCH_M60_REAL_COUNTER_TRUSTED_LABEL_ALLOWED_SAME_UID_UNCONFINED_DENIED",
              flush=True)

        restricted, _ = client(gateway, public, "add", 1, epoch,
                               mode="root-attacker-profile", label=attacker)
        expected_error(restricted, "untrusted_kernel_client_domain",
                       uid=ALLOWED_UID, label=attacker)
        print("RESEARCH_M60_SAME_UID_CONFINED_ATTACKER_LABEL_REJECTED",
              flush=True)

        # This is a *real* OS domain-transition check. Require security
        # refusal, not merely an arbitrary nonzero exit due to a bad test.
        switch = cmd(sudo(
            "aa-exec", "-p", attacker, "--",
            "/usr/bin/setpriv", "--reuid", str(ALLOWED_UID),
            "--regid", str(ALLOWED_UID), "--clear-groups",
            "/usr/bin/aa-exec", "-p", trusted, "--",
            "/usr/bin/cat", "/proc/self/attr/current"
        ), check=False)
        if switch.returncode == 0:
            raise AssertionError(
                "restricted same-UID attacker entered trusted OS profile")
        if not any(x in switch.stderr.lower() for x in (
            "permission denied", "operation not permitted",
            "insufficient permissions to change to the profile",
        )):
            raise AssertionError(
                f"AppArmor profile switch failed for unrelated reason: {switch.stderr!r}")
        print("RESEARCH_M60_CONFINED_SAME_UID_TRUSTED_DOMAIN_SWITCH_DENIED",
              flush=True)

        # Compare a client under a DIFFERENT UID but trusted profile.
        # BOTH OS principal UID and OS domain admission remain necessary
        # under the test policy.
        foreign, _ = client(gateway, public, "add", 1, epoch,
                            mode="root-profile", label=trusted,
                            uid=OUTSIDER_UID, claimed_uid=ALLOWED_UID)
        expected_error(foreign, "not_authorized", uid=OUTSIDER_UID)
        print("RESEARCH_M60_TRUSTED_LABEL_WRONG_UID_STILL_DENIED",
              flush=True)

        # Material negative control: the *unconfined* same-UID attacker is
        # not guaranteed to be restricted from selecting an installed
        # permissive AppArmor profile. Prior #129 showed a real bypass.
        attack, attempted = client(gateway, public, "add", 3, epoch,
                                   mode="unconfined-self-enter", label=trusted)
        if attack is None:
            if not any(x in attempted.stderr.lower() for x in (
                "permission denied", "operation not permitted",
                "insufficient permissions",
            )):
                raise AssertionError(
                    f"unconfined self-entry did not prove OS denial: "
                    f"rc={attempted.returncode} err={attempted.stderr[-750:]!r}"
                )
            expected_total = 2
            print("RESEARCH_M60_UNCONFINED_SELF_ENTRY_OS_DENIED_ON_THIS_HOST",
                  flush=True)
        else:
            expected_value(attack, 5)
            expected_total = 5
            print("RESEARCH_M60_UNCONFINED_SAME_UID_SELF_ENTERS_TRUSTED_LABEL_AND_MUTATES_REAL_COUNTER",
                  flush=True)

        final, _ = client(gateway, public, "value", 0, epoch,
                          mode="root-profile", label=trusted)
        expected_value(final, expected_total)
        original.run_admin(gateway, control, {
            "action": "revoke", "uid": ALLOWED_UID
        })
        revoked, _ = client(gateway, public, "add", 1, epoch,
                            mode="root-profile", label=trusted)
        expected_error(revoked, "not_authorized", uid=ALLOWED_UID)
        print("RESEARCH_M60_LABEL_PLUS_UID_REVOCATION_BLOCKS_NEW_MUTATIONS",
              flush=True)
        print("RESEARCH_M60_ACTUAL_KERNEL_DOMAIN_COUNTER_RESEARCH_PASS",
              flush=True)
    finally:
        if started:
            cmd(sudo("systemctl", "--system", "--no-pager", "stop",
                     safe_unit(unit)), check=False)
            cmd(sudo("systemctl", "--system", "--no-pager", "reset-failed",
                     safe_unit(unit)), check=False)
        if loaded:
            cmd(sudo("apparmor_parser", "-R", str(policy)))
        if created_runtime:
            cmd(sudo("rm", "-rf", "--", str(safe_runtime(runtime))))
        if created_install:
            cmd(sudo("rm", "-rf", "--", str(safe_install(install))))
        shutil.rmtree(srcdir)


if __name__ == "__main__":
    if len(sys.argv) != 4 or sys.argv[1] != "--exercise":
        raise SystemExit("usage: probe.py --exercise <pinned #149 source> <pinned #90 source>")
    exercise(Path(sys.argv[2]).resolve(), Path(sys.argv[3]).resolve())
