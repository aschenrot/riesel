#!/usr/bin/env python3
"""Research-only Linux process/control probe for Riesel #59.

Stdlib only. Models selected Manager policies but DOES NOT exercise Riesel,
Windows jobs or systemd. Its controller is deliberately disposable.
"""
import fcntl
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import sys
import tempfile
import time
import unittest

SELF = str(Path(__file__).resolve())


def birth(pid):
    try:
        s = Path(f"/proc/{pid}/stat").read_text().split(") ", 1)[1].split()
        return s[19] if s[0] not in ("Z", "X") else None
    except (FileNotFoundError, ProcessLookupError):
        return None


def alive(pid):
    return birth(pid) is not None


def until(predicate, limit=3):
    end = time.monotonic() + limit
    while time.monotonic() < end:
        if predicate():
            return True
        time.sleep(0.02)
    return bool(predicate())


def read_marker(path):
    try:
        return json.loads(Path(path).read_text())
    except (OSError, ValueError):
        return None


def app(mode, marker):
    if mode == "cooperative":
        def terminate(_sig, _frame):
            Path(marker + ".stopped").write_text("graceful")
            sys.exit(0)
        signal.signal(signal.SIGTERM, terminate)
    elif mode == "stubborn":
        signal.signal(signal.SIGTERM, signal.SIG_IGN)
    descendant = None
    if mode == "descendant":
        descendant = subprocess.Popen(
            [sys.executable, SELF, "--app", "cooperative", marker + ".desc"],
            start_new_session=True, stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if not until(lambda: read_marker(marker + ".desc") is not None):
            raise RuntimeError("descendant did not start")
    Path(marker).write_text(json.dumps({"pid": os.getpid(), "descendant": descendant.pid if descendant else None}))
    while True:
        time.sleep(0.2)


def spawn(mode, marker):
    return subprocess.Popen([sys.executable, SELF, "--app", mode, marker],
                            start_new_session=True, stdin=subprocess.DEVNULL,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def controller(mode, marker, lock=""):
    if mode == "lock":
        handle = open(lock, "a+")
        fcntl.flock(handle, fcntl.LOCK_EX)
    spawn("cooperative", marker)
    if not until(lambda: read_marker(marker) is not None):
        os._exit(91)
    if mode == "crash":
        os._exit(23)  # no child cleanup; app has its own session
    while True:
        time.sleep(0.2)


def reconcile(marker):
    observation = read_marker(marker)
    if observation and alive(observation["pid"]):
        print("unreconciled")  # not an OS process-owner handle
    else:
        print("not_running")


def command_server(sockpath, journal):
    server = socket.socket(socket.AF_UNIX)
    server.bind(sockpath)
    server.listen(3)
    for _ in range(3):
        conn, _ = server.accept()
        with conn:
            req = conn.recv(256).decode().strip().split(" ")
            if req[0] == "run":
                # Minimal durable fixture; not a transaction or production journal.
                p = Path(journal)
                existing = read_marker(journal) or {}
                existing.setdefault(req[1], {"result": "completed", "effects": 1})
                with p.open("w") as f:
                    json.dump(existing, f)
                    f.flush()
                    os.fsync(f.fileno())
                # Deliberately drop the reply *after* applying the effect.
            elif req[0] == "status":
                conn.sendall(json.dumps((read_marker(journal) or {}).get(req[1], {})).encode())
            elif req[0] == "peer":
                pid, uid, gid = struct.unpack("3i", conn.getsockopt(socket.SOL_SOCKET, socket.SO_PEERCRED, 12))
                conn.sendall(json.dumps({"pid": pid, "uid": uid, "gid": gid}).encode())
    server.close()


def send(sockpath, request):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(2)
        s.connect(sockpath)
        s.sendall(request.encode())
        s.shutdown(socket.SHUT_WR)
        return s.recv(1024)


def stop(p, force=False):
    if p and p.poll() is None:
        (p.kill() if force else p.terminate())
        try:
            p.wait(timeout=1)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait(timeout=2)


def kill_pid(pid, expected_birth):
    if pid and expected_birth and birth(pid) == expected_birth:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        until(lambda: not alive(pid))


@unittest.skipUnless(sys.platform == "linux" and Path("/proc/self/stat").exists(), "Linux /proc required")
class ManagerOwnershipEvidence(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="riesel-59-")
        self.root = Path(self.tmp.name)
        self.pids = {}
        self.procs = []

    def tearDown(self):
        for p in self.procs:
            stop(p, force=True)
        for pid, expected_birth in self.pids.items():
            kill_pid(pid, expected_birth)
        self.tmp.cleanup()

    def mark(self, name):
        return str(self.root / name)

    def launch(self, mode, name):
        marker = self.mark(name)
        p = spawn(mode, marker)
        self.procs.append(p)
        self.assertTrue(until(lambda: read_marker(marker) is not None), name)
        self.pids[p.pid] = birth(p.pid)
        return p, marker

    def test_01_owned_direct_child_start_stop(self):
        p, marker = self.launch("cooperative", "app")
        self.assertTrue(alive(p.pid))
        p.terminate()
        self.assertEqual(p.wait(timeout=2), 0)
        self.assertEqual(Path(marker + ".stopped").read_text(), "graceful")

    def test_02_manager_crash_does_not_clean_detached_child(self):
        marker = self.mark("abandoned")
        owner = subprocess.run([sys.executable, SELF, "--controller", "crash", marker], timeout=4)
        self.assertEqual(owner.returncode, 23)
        pid = read_marker(marker)["pid"]
        self.pids[pid] = birth(pid)
        self.assertTrue(alive(pid))

    def test_03_restart_reconciliation_must_not_adopt_pid(self):
        marker = self.mark("orphan")
        owner = subprocess.run([sys.executable, SELF, "--controller", "crash", marker], timeout=4)
        self.assertEqual(owner.returncode, 23)
        pid = read_marker(marker)["pid"]
        self.pids[pid] = birth(pid)
        restarted = subprocess.check_output([sys.executable, SELF, "--reconcile", marker], timeout=3)
        self.assertEqual(restarted.decode().strip(), "unreconciled")
        self.assertTrue(alive(pid))  # no unsafe adoption or stop

    def test_04_top_level_stop_leaves_escaped_descendant(self):
        parent, marker = self.launch("descendant", "tree")
        child = read_marker(marker)["descendant"]
        self.pids[child] = birth(child)
        self.assertTrue(alive(child))
        parent.kill()
        parent.wait(timeout=2)
        self.assertTrue(alive(child), "child escaped in its own session")

    def test_05_stale_or_wrong_owner_generation_refused(self):
        p, _ = self.launch("cooperative", "fenced")
        record = {"registration": "app-a", "owner": "manager-a", "generation": "g1",
                  "pid": p.pid, "birth": birth(p.pid)}
        def valid(request):
            return all(request.get(k) == record[k] for k in ("registration", "owner", "generation", "birth")) and birth(record["pid"]) == record["birth"]
        for patch in ({"owner": "manager-b"}, {"generation": "g0"}, {"birth": "invalid"}, {"registration": "app-b"}):
            self.assertFalse(valid({**record, **patch}))
            self.assertTrue(alive(p.pid))
        self.assertTrue(valid(record))
        # Policy-model assertion, not a Riesel production stop API.

    def test_06_controller_lock_does_not_prevent_duplicate_after_crash(self):
        lock, marker = self.mark("lock"), self.mark("first")
        owner = subprocess.Popen([sys.executable, SELF, "--controller", "lock", marker, lock],
                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.procs.append(owner)
        self.assertTrue(until(lambda: read_marker(marker) is not None))
        first = read_marker(marker)["pid"]
        self.pids[first] = birth(first)
        # A live controller's lock prevents a second controller lock.
        with open(lock, "a+") as competitor:
            with self.assertRaises(BlockingIOError):
                fcntl.flock(competitor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        owner.kill()
        owner.wait(timeout=2)
        self.assertTrue(alive(first))
        with open(lock, "a+") as f:
            fcntl.flock(f, fcntl.LOCK_EX | fcntl.LOCK_NB)
            other, _ = self.launch("cooperative", "second")
            self.assertTrue(alive(first) and alive(other.pid))
            fcntl.flock(f, fcntl.LOCK_UN)

    def test_07_graceful_shutdown_then_bounded_escalation(self):
        graceful, marker = self.launch("cooperative", "graceful")
        graceful.terminate()
        self.assertEqual(graceful.wait(timeout=2), 0)
        self.assertTrue(Path(marker + ".stopped").exists())
        stubborn, _ = self.launch("stubborn", "stubborn")
        stubborn.terminate()
        with self.assertRaises(subprocess.TimeoutExpired):
            stubborn.wait(timeout=0.2)
        stubborn.kill()
        self.assertEqual(stubborn.wait(timeout=2), -signal.SIGKILL)

    def test_08_lost_command_ack_requires_status_query(self):
        path, journal = self.mark("control.sock"), self.mark("journal.json")
        server = subprocess.Popen([sys.executable, SELF, "--command-server", path, journal],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.procs.append(server)
        self.assertTrue(until(lambda: Path(path).exists()))
        self.assertEqual(send(path, "run cmd-1"), b"")
        self.assertEqual(json.loads(send(path, "status cmd-1")), {"result": "completed", "effects": 1})
        self.assertEqual(read_marker(journal)["cmd-1"]["effects"], 1)
        # No blind replay; a lost response is an unknown outcome to the caller.

    def test_09_same_uid_peer_credentials_not_per_client_authorization(self):
        path, journal = self.mark("peer.sock"), self.mark("journal.json")
        server = subprocess.Popen([sys.executable, SELF, "--command-server", path, journal],
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.procs.append(server)
        self.assertTrue(until(lambda: Path(path).exists()))
        ours = json.loads(send(path, "peer"))
        sibling = json.loads(subprocess.check_output([sys.executable, SELF, "--peer", path], timeout=3))
        self.assertEqual(ours["uid"], sibling["uid"])
        self.assertNotEqual(ours["pid"], sibling["pid"])


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--app":
        app(sys.argv[2], sys.argv[3])
    elif len(sys.argv) > 1 and sys.argv[1] == "--controller":
        controller(sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else "")
    elif len(sys.argv) > 1 and sys.argv[1] == "--reconcile":
        reconcile(sys.argv[2])
    elif len(sys.argv) > 1 and sys.argv[1] == "--command-server":
        command_server(sys.argv[2], sys.argv[3])
    elif len(sys.argv) > 1 and sys.argv[1] == "--peer":
        print(send(sys.argv[2], "peer").decode())
    else:
        unittest.main(verbosity=2)
