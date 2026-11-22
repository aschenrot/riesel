#!/usr/bin/env python3
"""Research only (#60/#75): real Riesel Counter behind Linux OS-isolated export.

A disposable systemd service (PrivateNetwork=yes) runs a genuine, unmodified
SDK-resolved Counter under a separate numeric Linux UID and serves an AF_UNIX
application-facing research gateway. The controller grants *UID-scoped*
operation permissions via a root-only test control socket. The public socket
is deliberately 0666 to test kernel SO_PEERCRED admission, not path hiding.

NOT a production exporter, authenticated individual GUI identity, Manager,
portable transport, or Riesel-issued runtime-session generation. A second
process under a granted UID gets the same privileges: this negative control
is intentional and must never be described as secure same-UID isolation.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import secrets
import signal
import socket
import socketserver
import struct
import subprocess
import sys
import threading
import time
import uuid

APP_UID = 60092
MAX_REQ = 8192
CAP = {"id": "riesel.counter.async.operations", "version": 1}
APP = "research.private.counter"
INTERFACE = "ui.counter.v1"
DIR_PATTERN = re.compile(r"/opt/riesel-m60-protected-export-[a-f0-9]{12}")
RUN_PATTERN = re.compile(r"/run/riesel-m60-protected-export-[a-f0-9]{12}")


def exact_paths(install, runtime):
    install, runtime = Path(install), Path(runtime)
    if not DIR_PATTERN.fullmatch(str(install)) or not RUN_PATTERN.fullmatch(str(runtime)):
        raise AssertionError("refusing non-research service paths")
    return install, runtime


def peer(sock):
    pid, uid, gid = struct.unpack(
        "3i", sock.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12)
    )
    return {"pid": pid, "uid": uid, "gid": gid}


def exchange(sock, request):
    sock.sendall(json.dumps(request, separators=(",", ":")).encode() + b"\n")
    frame = bytearray()
    while b"\n" not in frame:
        part = sock.recv(4096)
        if not part:
            raise EOFError("response lost; mutation outcome UNKNOWN")
        frame.extend(part)
        if len(frame) > MAX_REQ:
            raise AssertionError("oversize response")
    return json.loads(frame.split(b"\n", 1)[0])


def call_counter(endpoint, operation, arguments=None):
    with socket.create_connection(endpoint, timeout=3) as sock:
        sock.settimeout(3)
        answer = exchange(
            sock,
            {"version": 1, "capability": CAP, "operation": operation,
             "arguments": arguments or {}},
        )
    value = answer.get("result", {})
    if not isinstance(value, dict) or not isinstance(value.get("Ok"), dict):
        raise AssertionError(f"actual Counter rejected RPC {operation}: {answer}")
    return value["Ok"]


def dropped_app_uid():
    os.setgroups([])
    os.setgid(APP_UID)
    os.setuid(APP_UID)


def proc_uid(pid):
    lines = Path(f"/proc/{pid}/status").read_text().splitlines()
    return int(next(x for x in lines if x.startswith("Uid:")).split()[1])


def read_request(sock):
    frame = bytearray()
    sock.settimeout(5)
    while b"\n" not in frame:
        data = sock.recv(2048)
        if not data:
            raise EOFError("request closed")
        frame.extend(data)
        if len(frame) > MAX_REQ:
            raise ValueError("request exceeds hard research limit")
    return json.loads(frame.split(b"\n", 1)[0])


class Gateway:
    def __init__(self, app, endpoint, runtime):
        self.app = app
        self.endpoint = endpoint
        self.runtime = runtime
        self.epoch = uuid.uuid4().hex  # Gateway-issued TEST epoch, NOT Riesel runtime session
        self.allowed = {}
        self.lock = threading.RLock()
        self.pause_next = False
        self.paused = threading.Event()
        self.release = threading.Event()
        self.closed = False

    def client(self, request, credential):
        op = request.get("operation")
        if op == "descriptor":
            return {"app": APP, "interface": INTERFACE,
                    "epoch": self.epoch, "ready": True}
        if request.get("app") != APP or request.get("interface") != INTERFACE:
            return {"error": "wrong_application_or_interface"}
        if request.get("epoch") != self.epoch:
            return {"error": "stale_gateway_epoch"}
        if op not in ("value", "add"):
            return {"error": "not_published"}
        if op == "add" and request.get("arguments") != {"amount": request.get("arguments", {}).get("amount")}:
            return {"error": "invalid_arguments"}
        if op == "add":
            amount = request.get("arguments", {}).get("amount")
            if type(amount) is not int or not 1 <= amount <= 20:
                return {"error": "invalid_arguments"}
        if self.pause_next:
            # Deterministic research-only hook: pause BEFORE authorization
            # and dispatch, and let privileged coordinator revoke.
            with self.lock:
                if self.pause_next:
                    self.pause_next = False
                    self.paused.set()
                    will_pause = True
                else:
                    will_pause = False
            if will_pause and not self.release.wait(8):
                return {"error": "test_gate_timeout"}
        with self.lock:
            if op not in self.allowed.get(credential["uid"], set()):
                return {"error": "not_authorized", "observed_uid": credential["uid"]}
            try:
                # Prevent the admin revocation from returning while an
                # already admitted synchronous request is dispatching.
                # Does NOT provide async bounded cancellation/unknown effect.
                result = call_counter(self.endpoint, op, request.get("arguments"))
            except (OSError, EOFError, TimeoutError) as exc:
                return {"error": "outcome_unknown", "detail": type(exc).__name__}
            return {"result": result, "observed_uid": credential["uid"]}

    def admin(self, request):
        with self.lock:
            kind = request.get("action")
            if kind == "status":
                return {"ready": True, "epoch": self.epoch,
                        "app_pid": self.app.pid,
                        "backend": f"{self.endpoint[0]}:{self.endpoint[1]}",
                        "paused": self.paused.is_set(),
                        "app_uid": proc_uid(self.app.pid)}
            if kind == "grant":
                uid = request.get("uid")
                ops = request.get("operations")
                if (type(uid) is not int or uid not in (60090, 60091)
                        or not isinstance(ops, list)
                        or not ops or not set(ops) <= {"value", "add"}):
                    return {"error": "invalid_test_grant"}
                self.allowed[uid] = set(ops)
                return {"ok": True, "operations": sorted(self.allowed[uid])}
            if kind == "revoke":
                uid = request.get("uid")
                if uid not in (60090, 60091):
                    return {"error": "invalid_test_revoke"}
                self.allowed.pop(uid, None)
                return {"ok": True}
            if kind == "arm_pause":
                self.paused.clear()
                self.release.clear()
                self.pause_next = True
                return {"ok": True}
            if kind == "resume":
                self.release.set()
                return {"ok": True}
            return {"error": "invalid_admin_action"}


class UdsServer(socketserver.ThreadingUnixStreamServer):
    daemon_threads = True
    request_queue_size = 8


def server_loop(gate, path, admin=False):
    class Handler(socketserver.BaseRequestHandler):
        def handle(self):
            real = peer(self.request)
            try:
                request = read_request(self.request)
                if admin:
                    result = (gate.admin(request) if real["uid"] == 0
                              else {"error": "not_admin"})
                else:
                    result = gate.client(request, real)
            except (ValueError, KeyError, EOFError, json.JSONDecodeError):
                result = {"error": "invalid_request"}
            try:
                self.request.sendall(
                    json.dumps(result, separators=(",", ":")).encode() + b"\n"
                )
            except (BrokenPipeError, ConnectionResetError):
                pass
    if path.exists():
        raise AssertionError(f"refuse to overwrite existing UDS socket: {path}")
    server = UdsServer(str(path), Handler)
    os.chmod(path, 0o600 if admin else 0o666)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def serve(install, runtime):
    if os.geteuid() != 0:
        raise AssertionError("separate protected system-owner test service required")
    install, runtime = exact_paths(install, runtime)
    if os.stat("/proc/self/ns/net").st_ino == os.stat("/proc/1/ns/net").st_ino:
        raise AssertionError("PrivateNetwork=yes not enforced; refuse unsafe exposure")
    if runtime.stat().st_uid != 0 or runtime.stat().st_mode & 0o022:
        raise AssertionError("research runtime directory not root-owned/protected")
    binary = install / "counter_resolved_facade_host"
    if not binary.is_file():
        raise AssertionError("pinned Counter binary not installed")
    app = subprocess.Popen(
        [str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT, text=True, cwd="/tmp", bufsize=1,
        preexec_fn=dropped_app_uid,
    )
    gateway = None
    servers = []
    try:
        line = []
        def wait_bound():
            line.append(app.stdout.readline())
        reader = threading.Thread(target=wait_bound, daemon=True)
        reader.start()
        reader.join(timeout=12)
        if not line or not line[0].startswith("RESEARCH_FACADE_BOUND "):
            raise AssertionError(f"real protected Counter not bound: {line}")
        address = line[0].split()[-1].strip()
        if not re.fullmatch(r"127\.0\.0\.1:[0-9]{1,5}", address):
            raise AssertionError("real Counter published unexpected endpoint")
        endpoint = ("127.0.0.1", int(address.rsplit(":", 1)[1]))
        if proc_uid(app.pid) != APP_UID:
            raise AssertionError("genuine Counter lacks isolated real OS UID")
        if call_counter(endpoint, "value") != {"Ok": 0}:
            raise AssertionError("real Counter not callable within private namespace")
        gateway = Gateway(app, endpoint, runtime)
        admin_dir = runtime / "owner"
        admin_dir.mkdir(mode=0o700)
        os.chmod(admin_dir, 0o700)
        servers.append(server_loop(gateway, admin_dir / "control.sock", admin=True))
        servers.append(server_loop(gateway, runtime / "public.sock"))
        status_path = runtime / "status.json"
        status_tmp = runtime / "status.partial"
        status_tmp.write_text(
            json.dumps({"gateway_ready": True, "app_pid": app.pid,
                        "app_uid": APP_UID, "epoch": gateway.epoch,
                        "backend": address})
        )
        os.chmod(status_tmp, 0o644)
        status_tmp.replace(status_path)
        print("RESEARCH_M60_REAL_RIESEL_PRIVATE_NET_PUBLISHED_GATEWAY", flush=True)
        stop = threading.Event()
        signal.signal(signal.SIGTERM, lambda *_: stop.set())
        while not stop.wait(0.25):
            if app.poll() is not None:
                raise AssertionError("real app exited unexpectedly")
    finally:
        for server, thread in servers:
            server.shutdown()
            server.server_close()
            thread.join(timeout=3)
        if app.poll() is None:
            try:
                app.stdin.write("STOP\n")
                app.stdin.flush()
                app.wait(timeout=5)
            except (BrokenPipeError, OSError, subprocess.TimeoutExpired):
                app.kill()
                app.wait(timeout=5)


def uds_query(path, payload, expected_server=0):
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as sock:
        sock.settimeout(8)
        sock.connect(path)
        actual = peer(sock)
        if actual["uid"] != expected_server:
            raise AssertionError(
                f"client sees wrong kernel server UID: {actual['uid']}")
        return exchange(sock, payload)


def client(path, op, amount, epoch, claimed_uid):
    return uds_query(path, {
        "app": APP, "interface": INTERFACE, "epoch": epoch,
        "operation": op, "arguments": {"amount": amount} if op == "add" else {},
        "claimed_uid": claimed_uid,  # Intentionally not trusted.
    })


def admin(path, request):
    if os.geteuid() != 0:
        raise AssertionError("root test approval, not GUI authority")
    return uds_query(path, request)


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--serve":
        serve(sys.argv[2], sys.argv[3])
    elif len(sys.argv) == 4 and sys.argv[1] == "--admin":
        print(json.dumps(admin(sys.argv[2], json.loads(sys.argv[3]))), flush=True)
    elif len(sys.argv) == 7 and sys.argv[1] == "--client":
        print(json.dumps(client(
            sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5],
            int(sys.argv[6]),
        )), flush=True)
    else:
        raise SystemExit("invalid isolated-export research invocation")


if __name__ == "__main__":
    main()
