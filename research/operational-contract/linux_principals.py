#!/usr/bin/env python3
"""Linux-only #75 threat-model comparator: real per-UID Unix socket privileges.

Requires sudo -n and setpriv on an isolated disposable CI runner. This is NOT
Riesel authentication, an issuer, a broker implementation or a production
credential service. Does not install users, services or persistent credentials.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import time

SERVER_UID = 60091
OUTSIDER_UID = 60092
SELF = Path(__file__).resolve()


def credentials(conn):
    return dict(zip(("pid", "uid", "gid"), struct.unpack(
        "3i", conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
    )))


def broker(socket_path: Path, secret_dir: Path, client_uid: int):
    assert os.getuid() == SERVER_UID, "privileged service UID setup failed"
    secret_dir.mkdir(mode=0o700)
    os.chmod(secret_dir, 0o700)
    (secret_dir / "identity").write_text("isolated-server-owner-secret")
    os.chmod(secret_dir / "identity", 0o600)
    with socket.socket(socket.AF_UNIX) as listener:
        listener.settimeout(12)
        listener.bind(str(socket_path))
        os.chmod(socket_path, 0o666)  # Test authorization in code, not path mode.
        listener.listen(8)
        for _ in range(3):
            conn, _ = listener.accept()
            with conn:
                conn.settimeout(3)
                req = json.loads(conn.recv(1024))
                real = credentials(conn)
                allow = req["operation"] == "mutate" and real["uid"] == client_uid
                conn.sendall(json.dumps({
                    "accepted": allow,
                    "observed_pid": real["pid"],
                    "observed_uid": real["uid"],
                    "claimed_uid": req.get("claimed_uid"),
                }).encode())


def client(socket_path: Path, operation: str, claimed_uid: int, expected_server_uid: int):
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(5)
        conn.connect(str(socket_path))
        observed_server = credentials(conn)
        if observed_server["uid"] != expected_server_uid:
            print(json.dumps({
                "accepted": False, "error": "wrong_server_principal",
                "observed_server_uid": observed_server["uid"]
            }))
            return
        conn.sendall(json.dumps({
            "operation": operation,
            "claimed_uid": claimed_uid
        }).encode())
        reply = json.loads(conn.recv(1024))
        reply["verified_server_uid"] = observed_server["uid"]
        print(json.dumps(reply))


def unreadable(path: Path):
    try:
        path.read_bytes()
    except PermissionError:
        return True
    return False


def run_client(program, socket_path, claimant_uid, caller_uid, server_uid, other=False):
    command = [sys.executable, str(program), "--client", str(socket_path),
               "mutate", str(claimant_uid), str(server_uid)]
    if other:
        command = ["sudo", "-n", "setpriv", "-u", str(caller_uid),
                   "-g", str(caller_uid), "--clear-groups", *command]
    result = subprocess.run(
        command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, timeout=15,
    )
    if result.returncode:
        raise AssertionError(f"client failed: {command}, {result.stderr}")
    return json.loads(result.stdout.strip())


def verify():
    if not sys.platform.startswith("linux") or not hasattr(socket, "SO_PEERCRED"):
        raise RuntimeError("Linux SO_PEERCRED required, not a portable guarantee")
    if os.getuid() == 0:
        raise RuntimeError("launch test from unprivileged CI user, not root")
    for executable in ("sudo", "setpriv", "python3"):
        if shutil.which(executable) is None:
            raise RuntimeError(f"missing real OS isolation prerequisite: {executable}")
    probe = subprocess.run(["sudo", "-n", "true"], capture_output=True, text=True)
    if probe.returncode:
        raise RuntimeError("dedicated kernel principal test requires non-interactive sudo")
    from pwd import getpwuid
    for reserved in (SERVER_UID, OUTSIDER_UID):
        try:
            existing = getpwuid(reserved)
        except KeyError:
            continue
        raise RuntimeError(f"refusing existing UID {reserved}: {existing.pw_name}")

    user_uid = os.getuid()
    with tempfile.TemporaryDirectory(prefix="riesel-s2-real-principal-") as folder:
        root = Path(folder)
        program = root / "s2.py"
        shutil.copyfile(SELF, program)
        os.chmod(program, 0o644)
        os.chmod(root, 0o711)
        socket_dir = root / "sockets"
        socket_dir.mkdir(mode=0o777)
        os.chmod(socket_dir, 0o777)
        socket_path = socket_dir / "broker.sock"
        secret = socket_dir / "private" / "identity"

        server_command = [
            "sudo", "-n", "setpriv", "-u", str(SERVER_UID),
            "-g", str(SERVER_UID), "--clear-groups",
            sys.executable, str(program), "--server",
            str(socket_path), str(secret.parent), str(user_uid),
        ]
        server = subprocess.Popen(
            server_command, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            text=True,
        )
        try:
            deadline = time.monotonic() + 10
            while not socket_path.exists():
                if server.poll() is not None:
                    raise RuntimeError("broker exited: " + server.communicate()[1])
                if time.monotonic() >= deadline:
                    raise AssertionError("broker did not bind test-only Unix socket")
                time.sleep(0.02)

            legitimate = run_client(program, socket_path, user_uid, user_uid, SERVER_UID)
            sibling = run_client(program, socket_path, user_uid, user_uid, SERVER_UID)
            outsider = run_client(program, socket_path, user_uid, OUTSIDER_UID,
                                  SERVER_UID, other=True)
            assert legitimate["accepted"], legitimate
            assert sibling["accepted"], sibling
            assert legitimate["observed_pid"] != sibling["observed_pid"]
            assert legitimate["observed_uid"] == sibling["observed_uid"] == user_uid
            assert legitimate["verified_server_uid"] == SERVER_UID
            assert not outsider["accepted"], outsider
            assert outsider["observed_uid"] == OUTSIDER_UID
            assert outsider["claimed_uid"] == user_uid
            print("RESEARCH_S2_PEERCRED_KERNEL_UID_NOT_CLAIMED_UID", flush=True)
            print("RESEARCH_S2_SAME_UID_SIBLING_CAN_INVOKE_PRIVILEGED_ROLE", flush=True)

            # Server-owned 0700 directory and 0600 file separate the owner
            # secret from both the legitimate runner UID and an outsider UID.
            assert secret.parent.exists(), "server-owned private directory missing"
            assert unreadable(secret), "runner read isolated server-owned key"
            outsider_read = subprocess.run([
                "sudo", "-n", "setpriv", "-u", str(OUTSIDER_UID),
                "-g", str(OUTSIDER_UID), "--clear-groups",
                sys.executable, str(program), "--read-private", str(secret)
            ], capture_output=True, text=True, timeout=10)
            assert outsider_read.returncode == 13, outsider_read
            server_read = subprocess.run([
                "sudo", "-n", "setpriv", "-u", str(SERVER_UID),
                "-g", str(SERVER_UID), "--clear-groups",
                sys.executable, str(program), "--read-private", str(secret)
            ], capture_output=True, text=True, timeout=10)
            assert server_read.returncode == 0, server_read
            print("RESEARCH_S2_DISTINCT_UID_ISOLATES_SERVER_SECRET", flush=True)

            server.wait(timeout=10)
            assert server.returncode == 0, server.communicate()[1]
            print("RESEARCH_S2_REAL_LINUX_PRINCIPAL_DIFFERENTIAL_PASS", flush=True)
        finally:
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=5)
            # Unprivileged runner cannot unlink broker-owned secret directly.
            # Narrow cleanup on this ephemeral CI test-only root directory.
            subprocess.run(
                ["sudo", "-n", "rm", "-rf", "--", str(socket_dir / "private")],
                check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--server":
        broker(Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[4]))
    elif len(sys.argv) > 1 and sys.argv[1] == "--client":
        client(Path(sys.argv[2]), sys.argv[3], int(sys.argv[4]), int(sys.argv[5]))
    elif len(sys.argv) > 1 and sys.argv[1] == "--read-private":
        sys.exit(13 if unreadable(Path(sys.argv[2])) else 0)
    else:
        verify()
