#!/usr/bin/env python3
"""Reproducible Xvfb/native-event-loop research test; not production certification."""
import os
import queue
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BACKEND = ROOT / "target/debug/counter_resolved_facade_host"
GUI = ROOT / "research/native-gui-host/target/debug/riesel-native-gui-host-research"
PROCESSES = []


class Observed:
    def __init__(self, label, command):
        self.label = label
        self.proc = subprocess.Popen(
            [str(part) for part in command],
            cwd=ROOT,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            env=os.environ.copy(),
        )
        PROCESSES.append(self.proc)
        self.lines = queue.Queue()
        self.thread = threading.Thread(target=self._read, daemon=True)
        self.thread.start()
        print(f"{label}: pid={self.proc.pid}", flush=True)

    def _read(self):
        for line in self.proc.stdout:
            line = line.rstrip("\n")
            print(f"[{self.label}] {line}", flush=True)
            self.lines.put(line)

    def until(self, token, seconds=30):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if self.proc.poll() is not None and self.lines.empty():
                raise AssertionError(
                    f"{self.label} exited {self.proc.returncode} before {token!r}"
                )
            try:
                line = self.lines.get(timeout=min(0.2, max(0.01, end-time.monotonic())))
            except queue.Empty:
                continue
            if token in line:
                return line
        raise AssertionError(f"{self.label} did not report {token!r} in {seconds}s")

    def finish(self, seconds=15):
        code = self.proc.wait(timeout=seconds)
        if code != 0:
            raise AssertionError(f"{self.label} exited unsuccessfully: {code}")


def backend(label):
    app = Observed(label, [BACKEND])
    line = app.until("RESEARCH_FACADE_BOUND")
    match = re.search(r"RESEARCH_FACADE_BOUND (127\.0\.0\.1:\d+)", line)
    assert match, line
    return app, match.group(1)


def gui(label, address, delay_ms=0):
    client = Observed(label, [GUI, address, str(delay_ms)])
    client.until("GUI_PID")
    return client


def xdotool(*args):
    return subprocess.check_output(["xdotool", *map(str, args)], text=True).strip()


def find_window():
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        result = subprocess.run(
            ["xdotool", "search", "--onlyvisible", "--name", "Riesel native GUI research"],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode == 0 and result.stdout.strip():
            return result.stdout.splitlines()[-1]
        time.sleep(0.1)
    raise AssertionError("native GUI created no visible X11 window")


def focus(window):
    xdotool("windowfocus", "--sync", window)


def key(*keys):
    xdotool("key", "--clearmodifiers", *keys)


def close(client, window):
    xdotool("windowclose", window)
    client.until("GUI_CLOSED_WITHOUT_BACKEND_STOP", 15)
    client.finish()


def test_independent_lifecycle():
    app, address = backend("headless")
    first = gui("gui-first", address)
    assert first.proc.pid != app.proc.pid
    first.until("GUI_VALUE 0")
    window = find_window()
    focus(window)

    # Exercise real keyboard focus, not just synthetic invocation of SDK calls.
    for attempt in range(4):
        key("Tab")
        try:
            first.until("GUI_KEYBOARD_FOCUS", 1.5)
            break
        except AssertionError:
            if attempt == 3:
                raise

    for value in (1, 2, 3):
        key("ctrl+i")
        first.until(f"GUI_VALUE {value}")
    close(first, window)
    assert app.proc.poll() is None, "backend exited on GUI close"
    print("GUI_BACKEND_SURVIVED_CLOSE", flush=True)

    second = gui("gui-reopened", address)
    second.until("GUI_VALUE 3")
    other_window = find_window()
    focus(other_window)
    for value in (4, 5):
        key("ctrl+i")
        second.until(f"GUI_VALUE {value}")
    close(second, other_window)
    assert app.proc.poll() is None, "backend exited on GUI reopen/close"
    app.proc.stdin.write("STOP\n")
    app.proc.stdin.flush()
    app.finish()
    print("GUI_CLOSE_REOPEN_AUTHORITATIVE_STATE_PROVEN", flush=True)


def test_failure_and_responsiveness():
    app, address = backend("headless-failure")
    # Inject a worker-side delay to test a real event-loop response while
    # waiting. This is explicitly not a measured slow RPC server.
    client = gui("gui-failure", address, 1600)
    window = find_window()
    focus(window)
    key("h")
    client.until("GUI_INPUT_ALIVE", 6)
    client.until("GUI_VALUE 0", 20)
    app.proc.kill()
    app.proc.wait(timeout=10)
    client.until("GUI_DISCONNECTED", 20)
    focus(window)
    key("h")
    client.until("GUI_INPUT_ALIVE", 6)
    close(client, window)
    print("GUI_BACKEND_DISCONNECT_AND_INPUT_PROVEN", flush=True)


def main():
    if not os.environ.get("DISPLAY") or not shutil.which("xdotool"):
        raise SystemExit("X11 DISPLAY and xdotool are required")
    if not BACKEND.is_file() or not GUI.is_file():
        raise SystemExit("build both backend and GUI binaries before exercising")
    try:
        test_independent_lifecycle()
        test_failure_and_responsiveness()
        print("NATIVE_GUI_RESEARCH_XVFB_PASS", flush=True)
    finally:
        for proc in PROCESSES:
            if proc.poll() is None:
                proc.kill()
        for proc in PROCESSES:
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass


if __name__ == "__main__":
    main()
