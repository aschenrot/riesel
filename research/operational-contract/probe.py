#!/usr/bin/env python3
"""#75 research-only server-side gate in front of a real SDK-resolved Counter.

Not a Riesel SDK exporter, authorization guarantee, or runtime recovery test.
Run after cargo smoke: python3 research/operational-contract/probe.py --real
Run in model-only mode: python3 research/operational-contract/probe.py --model
"""
import argparse
import json
import os
import queue
import secrets
import socket
import socketserver
import subprocess
import sys
import tempfile
import threading
from pathlib import Path

CAP = {"id": "riesel.counter.async.operations", "version": 1}
APP = "research.counter.publisher"
INTERFACE = "counter.public"
MAX_FRAME = 1024 * 1024


def exchange(address, payload):
    with socket.create_connection(address, timeout=4) as connection:
        connection.settimeout(6)
        connection.sendall(json.dumps(payload, separators=(",", ":")).encode() + b"\n")
        frame = bytearray()
        while b"\n" not in frame:
            data = connection.recv(8192)
            if not data:
                raise EOFError("response lost after request submission; outcome unknown")
            frame += data
            if len(frame) > MAX_FRAME:
                raise AssertionError("unbounded response frame")
        return json.loads(frame.split(b"\n", 1)[0])


def rpc(address, operation, arguments=None):
    response = exchange(address, {"version": 1, "capability": CAP,
                                  "operation": operation, "arguments": arguments or {}})
    if "Ok" not in response.get("result", {}):
        raise AssertionError(f"backend RPC failure: {response}")
    return response["result"]["Ok"]  # Encoded Result<T, CounterError>


class RealHost:
    def __init__(self, binary):
        self.process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE,
                                        stdout=subprocess.PIPE, stderr=None, text=True)
        lines = queue.Queue(maxsize=1)
        threading.Thread(target=lambda: lines.put(self.process.stdout.readline()), daemon=True).start()
        try:
            announcement = lines.get(timeout=20).strip()
            assert announcement.startswith("RESEARCH_FACADE_BOUND "), announcement
            host, port = announcement.removeprefix("RESEARCH_FACADE_BOUND ").rsplit(":", 1)
            self.address = (host, int(port))
        except BaseException:
            self.abort()
            raise

    def stop(self):
        self.process.stdin.write("STOP\n")
        self.process.stdin.flush()
        try:
            status = self.process.wait(timeout=12)
        except subprocess.TimeoutExpired:
            self.abort()
            raise
        assert status == 0, f"Riesel resolved host failed shutdown: {status}"

    def abort(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait(timeout=5)


class ModelHost:
    """Only exercises the test apparatus where Rust/cargo are not installed."""
    def __init__(self, _):
        self.value = 0
        parent = self
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                request = read_frame(self.request)
                assert request["capability"] == CAP
                method = request["operation"]
                if method == "value":
                    answer = parent.value
                elif method == "add":
                    parent.value += request["arguments"]["amount"]
                    answer = parent.value
                else:
                    raise AssertionError(f"unexpected backend method {method}")
                self.request.sendall(json.dumps({"version": 1, "result": {"Ok": {"Ok": answer}}}).encode()+b"\n")
        self.server = BoundServer(("127.0.0.1", 0), Handler)
        self.address = self.server.server_address
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def stop(self):
        assert self.value == 5, f"expected final counter 5, got {self.value}"
        self.abort()

    def abort(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)


class BoundServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def read_frame(connection):
    frame = bytearray()
    connection.settimeout(6)
    while b"\n" not in frame:
        data = connection.recv(8192)
        if not data:
            raise EOFError("incomplete request")
        frame += data
        if len(frame) > MAX_FRAME:
            raise ValueError("request exceeds research bound")
    return json.loads(frame.split(b"\n", 1)[0])


class Gate:
    """Separately authored research adapter; NOT integrated into Riesel.

    The gate owns its epoch; a future SDK exporter would need to bind this
    atomically to the actual runtime session. This gate cannot prove that.
    """
    def __init__(self):
        self.lock = threading.RLock()
        self.backend = None
        self.generation = None
        self.attachments = {}
        self.read_proof = secrets.token_urlsafe(32)
        self.write_proof = secrets.token_urlsafe(32)
        self.ready = False
        self.callable = False
        outer = self
        class Handler(socketserver.BaseRequestHandler):
            def handle(self):
                req = read_frame(self.request)
                response = outer.request(req)
                if response is not None:  # deliberate lost response after dispatch
                    self.request.sendall(json.dumps(response).encode()+b"\n")
        self.server = BoundServer(("127.0.0.1", 0), Handler)
        self.address = self.server.server_address
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def status(self):
        with self.lock:
            return (self.server.socket.fileno() >= 0, self.ready, self.callable)

    def activate(self, backend_address):
        with self.lock:
            self.ready = False
            self.callable = False
            self.generation = secrets.token_hex(16)
            self.attachments.clear()
            self.backend = backend_address
            # A live query proves THIS operation on THIS backend is callable;
            # it is not evidence of whole-application health or identity.
            assert rpc(backend_address, "value") == {"Ok": 0}
            self.ready = True

    def request(self, request):
        with self.lock:
            if request.get("app") != APP:
                return {"error": "wrong_app"}
            if request.get("interface") != INTERFACE:
                return {"error": "wrong_interface"}
            if request.get("version") != 1:
                return {"error": "unsupported_version"}
            if request.get("type") == "attach":
                if not self.ready:
                    return {"error": "not_ready"}
                proof = request.get("proof")
                scope = ("write" if secrets.compare_digest(str(proof), self.write_proof)
                         else "read" if secrets.compare_digest(str(proof), self.read_proof)
                         else None)
                if scope is None:
                    return {"error": "unauthorized"}
                attachment = secrets.token_hex(24)
                self.attachments[attachment] = scope
                return {"generation": self.generation, "attachment": attachment, "scope": scope}
            if request.get("type") != "call":
                return {"error": "invalid_request"}
            if request.get("generation") != self.generation:
                return {"error": "stale_generation"}
            attachment = request.get("attachment")
            scope = self.attachments.get(attachment)
            if scope is None:
                return {"error": "unauthorized"}
            if not self.ready:
                return {"error": "not_ready"}
            operation = request.get("operation")
            if operation not in ("value", "add"):
                return {"error": "not_published"}
            if operation == "add" and scope != "write":
                return {"error": "forbidden"}
            result = rpc(self.backend, operation, request.get("arguments", {}))
            if operation == "value":
                self.callable = True
            if request.get("drop_reply"):
                return None
            return {"result": result}

    def stop(self):
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=5)


def message(kind, **fields):
    return {"type": kind, "app": APP, "interface": INTERFACE, "version": 1, **fields}


def attach(gate, proof):
    return exchange(gate.address, message("attach", proof=proof))


def invoke(gate, binding, operation, args=None, **fields):
    return exchange(gate.address, message("call", generation=binding["generation"],
                        attachment=binding["attachment"], operation=operation,
                        arguments=args or {}, **fields))


def assert_error(response, expected):
    assert response == {"error": expected}, (response, expected)


def demonstrate_same_uid_limit():
    if os.name != "posix" or not hasattr(os, "getuid"):
        print("SAME_UID_FILE_PERMISSION_TEST_SKIPPED non-POSIX")
        return
    with tempfile.TemporaryDirectory(prefix="riesel-same-uid-") as temporary:
        path = Path(temporary) / "credential"
        secret = secrets.token_hex(20)
        path.write_text(secret)
        path.chmod(0o600)
        output = subprocess.check_output([sys.executable, "-c",
            "from pathlib import Path; import sys; print(Path(sys.argv[1]).read_text())", str(path)], text=True)
        assert output.strip() == secret
    print("RESEARCH_SAME_UID_0600_SECRET_READABLE (no strong same-UID isolation)")


def run(binary):
    host_class = RealHost if binary else ModelHost
    gate = Gate()
    current = None
    try:
        assert gate.status() == (True, False, False)
        assert_error(attach(gate, gate.write_proof), "not_ready")
        current = host_class(binary)
        gate.activate(current.address)
        assert gate.status() == (True, True, False)
        assert_error(attach(gate, "wrong"), "unauthorized")
        reader = attach(gate, gate.read_proof)
        writer = attach(gate, gate.write_proof)
        assert reader["scope"] == "read" and writer["scope"] == "write"
        assert_error(invoke(gate, reader, "add", {"amount": 1}), "forbidden")
        assert_error(invoke(gate, {"attachment": "invalid", "generation": writer["generation"]},
                            "add", {"amount": 1}), "unauthorized")
        for mutation, expected in [({"app": "other"}, "wrong_app"),
                                   ({"interface": "private.counter"}, "wrong_interface"),
                                   ({"version": 2}, "unsupported_version")]:
            probe = message("call", generation=writer["generation"], attachment=writer["attachment"],
                            operation="add", arguments={"amount": 1})
            probe.update(mutation)
            assert_error(exchange(gate.address, probe), expected)
        assert invoke(gate, reader, "value")["result"] == {"Ok": 0}
        assert gate.status() == (True, True, True)
        assert_error(invoke(gate, writer, "observe"), "not_published")
        assert invoke(gate, writer, "add", {"amount": 5})["result"] == {"Ok": 5}
        current.stop()
        current = host_class(binary)
        gate.activate(current.address)  # listener remains at same public address
        assert_error(invoke(gate, writer, "add", {"amount": 3}), "stale_generation")
        assert rpc(current.address, "value") == {"Ok": 0}  # pre-dispatch rejection
        next_writer = attach(gate, gate.write_proof)
        assert next_writer["generation"] != writer["generation"]
        assert invoke(gate, next_writer, "add", {"amount": 2})["result"] == {"Ok": 2}
        assert invoke(gate, next_writer, "add", {"amount": 3})["result"] == {"Ok": 5}
        current.stop()
        print("RESEARCH_REAL_RESOLVED_COUNTER_GATE_REPLACEMENT_PASS" if binary else "RESEARCH_MODEL_GATE_REPLACEMENT_PASS")
        current = host_class(binary)
        gate.activate(current.address)
        before = attach(gate, gate.write_proof)
        # This rotates the adapter's generation in the same gateway PID;
        # it does not invoke Riesel's in-process whole-session recovery.
        gate.activate(current.address)
        assert_error(invoke(gate, before, "add", {"amount": 5}), "stale_generation")
        fresh = attach(gate, gate.write_proof)
        assert invoke(gate, fresh, "add", {"amount": 5})["result"] == {"Ok": 5}
        current.stop()
        print("RESEARCH_GATE_SAME_PID_ROTATION_MODEL_ONLY (NOT Riesel recovery)")
        current = host_class(binary)
        gate.activate(current.address)
        final_writer = attach(gate, gate.write_proof)
        try:
            invoke(gate, final_writer, "add", {"amount": 3}, drop_reply=True)
        except EOFError:
            pass  # Caller must reconcile; it is NOT safe to repeat add(3).
        else:
            raise AssertionError("expected lost response after a real backend mutation")
        assert invoke(gate, final_writer, "value")["result"] == {"Ok": 3}
        assert invoke(gate, final_writer, "add", {"amount": 2})["result"] == {"Ok": 5}
        current.stop()
        current = None
        print("RESEARCH_GATE_UNKNOWN_OUTCOME_NO_RETRY_PASS")
        demonstrate_same_uid_limit()
    finally:
        if current is not None:
            current.abort()
        gate.stop()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--real", action="store_true", help="requires cargo smoke binaries")
    group.add_argument("--model", action="store_true", help="test apparatus only, no Riesel")
    options = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    binary = root / "target" / "debug" / ("counter_resolved_facade_host" + (".exe" if os.name == "nt" else "")) if options.real else None
    if options.real and not binary.is_file():
        parser.error(f"required SDK-resolved Counter host binary not found: {binary}; run cargo smoke first")
    run(binary)
