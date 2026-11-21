#!/usr/bin/env python3
"""#59 research: executable provenance != caller intent or in-process code trust.

A root-owned, unchanged compiled "Manager CLI" is allowed by a disposable
root-side Unix-socket verifier using kernel SO_PEERCRED and /proc/PID/exe
identity. Both an intended invocation and two hostile same-UID invocations
are accepted; the last one also executes an attacker-controlled LD_PRELOAD
library inside the *same* verified executable.

Research-only: the server's only "privileged" action is incrementing a mock
counter. It NEVER touches systemd, Riesel application processes, credentials,
existing installations or user data. Runs on a disposable Linux CI runner.
"""
from __future__ import annotations

import hashlib
import os
from pathlib import Path
import re
import select
import shutil
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import uuid


CLIENT_C = r'''
#define _GNU_SOURCE
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(int argc, char **argv) {
    if (argc != 2) return 10;
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) return 11;
    struct sockaddr_un addr = {0};
    addr.sun_family = AF_UNIX;
    if (strlen(argv[1]) >= sizeof(addr.sun_path)) return 12;
    strncpy(addr.sun_path, argv[1], sizeof(addr.sun_path) - 1);
    if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) return 13;
    if (send(fd, "STOP\n", 5, 0) != 5) return 14;
    char reply[32] = {0};
    ssize_t n = recv(fd, reply, sizeof(reply) - 1, 0);
    if (n < 0) return 15;
    reply[n] = 0;
    fputs(reply, stdout);
    close(fd);
    return 0;
}
'''

INJECT_C = r'''
#include <unistd.h>
__attribute__((constructor)) static void injected(void) {
    static const char marker[] =
        "RESEARCH_M59_UNTRUSTED_LIBRARY_CODE_IN_VERIFIED_CLIENT\n";
    (void)write(2, marker, sizeof(marker) - 1);
}
'''


def cmd(args: list[str], timeout: int = 14, env=None):
    return subprocess.run(args, check=False, capture_output=True,
                          text=True, timeout=timeout, env=env, cwd="/tmp")


def checked(args: list[str], timeout: int = 14):
    res = cmd(args, timeout)
    if res.returncode:
        raise AssertionError(
            f"command failed {args[:4]!r}: {res.returncode}; "
            f"out={res.stdout[:500]!r}, err={res.stderr[:900]!r}"
        )
    return res


def sudo(*args: str):
    return ["sudo", "-n", *args]


def hash_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as handle:
        for data in iter(lambda: handle.read(65536), b""):
            h.update(data)
    return h.hexdigest()


def server_main(socket_path: Path, cli_path: Path, allowed_uid: int):
    if os.geteuid() != 0:
        raise AssertionError("research verifier must be independently privileged")
    expected = cli_path.stat()
    if expected.st_uid != 0 or stat.S_IMODE(expected.st_mode) & 0o022:
        raise AssertionError("research executable not protected from UID writes")
    original_hash = hash_file(cli_path)
    if socket_path.exists():
        raise AssertionError("research socket already exists")
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.settimeout(12)
    try:
        listener.bind(str(socket_path))
        os.chmod(socket_path, 0o666)  # Test intentionally admits all clients.
        listener.listen(4)
        print(f"READY {os.getpid()}", flush=True)
        approved, refused = 0, 0
        for _ in range(4):
            peer, _ = listener.accept()
            peer.settimeout(6)
            with peer:
                pid, uid, gid = struct.unpack(
                    "3i", peer.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
                )
                if pid <= 1 or uid == 0 or gid < 0:
                    raise AssertionError(f"unexpected kernel peer: {pid} {uid} {gid}")
                # Peer remains connected and alive while this observation is
                # taken; this is *research*, not race-free production admission.
                try:
                    observed = os.stat(f"/proc/{pid}/exe")
                except OSError as e:
                    raise AssertionError(f"peer exe observation failed: {e}") from e
                same_executable = (
                    observed.st_dev == expected.st_dev
                    and observed.st_ino == expected.st_ino
                    and hash_file(cli_path) == original_hash
                )
                request = peer.recv(32)
                allowed = uid == allowed_uid and same_executable and request == b"STOP\n"
                peer.sendall(b"ALLOW\n" if allowed else b"DENY\n")
                if allowed:
                    approved += 1
                else:
                    refused += 1
                print(
                    f"PEER_{approved + refused} uid_ok={uid == allowed_uid} "
                    f"root_executable_inode_ok={same_executable} "
                    f"allowed={allowed}",
                    flush=True,
                )
        if (approved, refused) != (3, 1):
            raise AssertionError(f"mock command counts wrong: {(approved, refused)}")
        print("RESEARCH_M59_ROOT_VERIFIER_MOCK_MUTATIONS_3_DENIED_1", flush=True)
        print("RESEARCH_M59_EXECUTABLE_IDENTITY_NOT_HUMAN_INTENT", flush=True)
    finally:
        listener.close()
        try:
            socket_path.unlink()
        except FileNotFoundError:
            pass


def direct_python(socket_path: Path) -> str:
    # Same UID, different executable => blocked by the deliberately naive
    # root-owned executable inode allowlist, despite correct SO_PEERCRED UID.
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(6)
    with sock:
        sock.connect(str(socket_path))
        sock.sendall(b"STOP\n")
        return sock.recv(32).decode().strip()


def runner():
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("requires disposable Linux nonroot coordinator")
    if not shutil.which("gcc"):
        raise AssertionError("gcc unavailable: test not executed")
    checked(sudo("true"))

    nonce = uuid.uuid4().hex[:12]
    protected = Path(f"/tmp/riesel-m59-executable-{nonce}")
    if not re.fullmatch(r"/tmp/riesel-m59-executable-[0-9a-f]{12}", str(protected)):
        raise AssertionError("unsafe fixture root")
    sources = Path(tempfile.mkdtemp(prefix="riesel-m59-build-"))
    script_src = Path(__file__).resolve()
    c_file = sources / "client.c"
    so_file = sources / "inject.c"
    compiled = sources / "manager-cli"
    library = sources / "attacker.so"
    c_file.write_text(CLIENT_C)
    so_file.write_text(INJECT_C)
    server = None
    server_pid = None
    created_protected = False
    try:
        checked(["gcc", "-std=c11", "-O2", "-Wall", "-Wextra",
                 str(c_file), "-o", str(compiled)])
        checked(["gcc", "-std=c11", "-fPIC", "-shared", "-Wall", "-Wextra",
                 str(so_file), "-o", str(library)])
        # One uniquely named root-owned 0755 directory, no service accounts,
        # no system-wide installed application files or policies.
        checked(sudo("mkdir", "--", str(protected)))
        created_protected = True
        checked(sudo("chmod", "0755", str(protected)))
        cli_path = protected / "manager-cli"
        installed_server = protected / "verifier.py"
        checked(sudo("install", "-o", "root", "-g", "root", "-m", "0755",
                     str(compiled), str(cli_path)))
        checked(sudo("install", "-o", "root", "-g", "root", "-m", "0644",
                     str(script_src), str(installed_server)))
        st = cli_path.stat()
        if st.st_uid != 0 or stat.S_IMODE(st.st_mode) != 0o755:
            raise AssertionError("root-owned CLI fixture incorrect")
        print("RESEARCH_M59_ROOT_OWNED_VERIFIED_CLI_PREPARED", flush=True)
        endpoint = protected / "control.sock"
        server = subprocess.Popen(
            sudo("/usr/bin/python3", "-u", str(installed_server), "--server",
                 str(endpoint), str(cli_path), str(os.getuid())),
            cwd="/tmp", stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            start_new_session=True,
        )
        assert server.stdout is not None
        readable, _, _ = select.select([server.stdout], [], [], 8)
        if not readable:
            raise AssertionError("root verifier did not start")
        line = server.stdout.readline().strip()
        if not re.fullmatch(r"READY [0-9]+", line):
            detail = server.stderr.read() if server.poll() is not None else ""
            raise AssertionError(f"server startup {line!r}, {detail[:800]!r}")
        server_pid = int(line.split()[1])
        if direct_python(endpoint) != "DENY":
            raise AssertionError("direct same-UID Python client incorrectly allowed")
        print("RESEARCH_M59_DIRECT_SAME_UID_UNTRUSTED_CLIENT_DENIED", flush=True)

        clean_env = os.environ.copy()
        clean_env.pop("LD_PRELOAD", None)
        legit = cmd([str(cli_path), str(endpoint)], env=clean_env)
        if legit.returncode != 0 or legit.stdout.strip() != "ALLOW":
            raise AssertionError(f"legit invocation failed {legit!r}")
        print("RESEARCH_M59_LEGIT_ROOT_OWNED_CLI_ALLOWED", flush=True)

        # This separate invocation represents the attacker simply executing
        # the unchanged trusted CLI. The broker cannot infer an operator
        # approved it from executable inode, UID or stable code digest.
        hostile_reexec = cmd([str(cli_path), str(endpoint)], env=clean_env)
        if hostile_reexec.returncode or hostile_reexec.stdout.strip() != "ALLOW":
            raise AssertionError(f"same-UID unapproved CLI invocation failed {hostile_reexec!r}")
        print("RESEARCH_M59_UNTRUSTED_SAME_UID_CAN_REEXEC_TRUSTED_CLI", flush=True)

        # Root-owned executable integrity alone doesn't imply no injected
        # code: this normal (non-setuid) CLI accepts LD_PRELOAD from the
        # invoking user's environment. Never load into a real privileged app.
        bad_env = clean_env.copy()
        bad_env["LD_PRELOAD"] = str(library)
        hostile_preloaded = cmd([str(cli_path), str(endpoint)], env=bad_env)
        marker = "RESEARCH_M59_UNTRUSTED_LIBRARY_CODE_IN_VERIFIED_CLIENT"
        if (hostile_preloaded.returncode != 0
                or hostile_preloaded.stdout.strip() != "ALLOW"
                or marker not in hostile_preloaded.stderr):
            raise AssertionError(
                f"expected isolated preload invocation did not execute: "
                f"out={hostile_preloaded.stdout!r}, err={hostile_preloaded.stderr!r}"
            )
        print("RESEARCH_M59_PRELOADED_SAME_UID_TRUSTED_EXE_STILL_ALLOWED", flush=True)
        if hash_file(cli_path) != hash_file(compiled):
            raise AssertionError("immutable root-owned executable changed")
        assert server is not None
        result = server.wait(timeout=10)
        details = server.stdout.read() if server.stdout else ""
        if result != 0 or "RESEARCH_M59_EXECUTABLE_IDENTITY_NOT_HUMAN_INTENT" not in details:
            err = server.stderr.read() if server.stderr else ""
            raise AssertionError(
                f"verifier failed rc={result}, stdout={details[-900:]!r}, "
                f"stderr={err[-900:]!r}"
            )
        print("RESEARCH_M59_VERIFIED_CLI_CODE_IDENTITY_INTENT_GAP_PASS", flush=True)
    finally:
        if server is not None and server.poll() is None:
            if server_pid is not None and server_pid > 1:
                cmd(sudo("kill", "-TERM", str(server_pid)), timeout=5)
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)
        if created_protected:
            # Only this unique, validated, disposable test directory.
            removed = cmd(sudo("rm", "-rf", "--", str(protected)))
            if removed.returncode or protected.exists():
                raise AssertionError(f"research root fixture not removed: {removed.stderr}")
        shutil.rmtree(sources)


if __name__ == "__main__":
    if len(sys.argv) == 1:
        runner()
    elif len(sys.argv) == 5 and sys.argv[1] == "--server":
        server_main(Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[4]))
    else:
        raise SystemExit("invalid research mode")
