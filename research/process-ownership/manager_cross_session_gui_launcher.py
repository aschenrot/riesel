#!/usr/bin/env python3
"""Disposable logind-session launcher for the unchanged PR #90 native GUI.

The already-running PAMName=login service invokes this file BEFORE its
session-bound Riesel Counter backend exists. Once a ROOT test fixture
publishes the address of that exact disposable backend, exec the genuine
eframe executable in the SAME process/logind session. This file does not
authenticate the address, issue a privileged Manager request or add any GUI.
"""
from __future__ import annotations

import os
from pathlib import Path
import re
import sys
import time


def main(root_text):
    root = Path(root_text)
    if not re.fullmatch(r"/opt/riesel-m59-crossgui-[a-f0-9]{12}", str(root)):
        raise AssertionError("invalid research-only install directory")
    if os.geteuid() == 0 or os.getuid() != os.geteuid():
        raise AssertionError("GUI launcher must belong to unprivileged login user")
    status = Path("/proc/self/status").read_text()
    effective = next(line for line in status.splitlines()
                     if line.startswith("CapEff:"))
    if int(effective.split(":")[1], 16):
        raise AssertionError("login GUI inherited Linux privileges")
    executable = root / "riesel-native-gui-host-research"
    if not executable.is_file():
        raise AssertionError("pinned original native GUI binary absent")
    address_file = root / "counter-endpoint"
    deadline = time.monotonic() + 55
    print(f"RESEARCH_SESSION_GUI_LAUNCHER_UID {os.getuid()}", flush=True)
    while time.monotonic() < deadline:
        try:
            address = address_file.read_text().strip()
        except FileNotFoundError:
            time.sleep(0.1)
            continue
        if not re.fullmatch(r"127\.0\.0\.1:[0-9]{1,5}", address):
            raise AssertionError("unexpected research-only loopback endpoint")
        port = int(address.rsplit(":", 1)[1])
        if not 1 <= port <= 65535:
            raise AssertionError("invalid ephemeral TCP listener")
        if not os.environ.get("DISPLAY"):
            raise AssertionError("Xvfb DISPLAY not propagated to PAM GUI")
        os.execv(executable, [str(executable), address])
    raise AssertionError("root test controller never published Counter endpoint")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: launcher.py </opt/riesel-m59-crossgui-12hex>")
    main(sys.argv[1])
