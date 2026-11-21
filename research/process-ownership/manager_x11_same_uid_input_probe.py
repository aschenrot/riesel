#!/usr/bin/env python3
"""#59/#60 research-only hostile same-UID X11 keyboard injection.

Executes as a separate ordinary desktop-user process in the disposable
Xvfb session of PR #134. It receives the already-visible test GUI's X11
window ID and PID, but deliberately receives NO app RPC address, Manager
credential, GUI client capability, or elevated OS permission.

This demonstrates a *desktop input boundary*, not an application export,
real protected Manager, polkit prompt, or Wayland behavior.
"""
from __future__ import annotations

import os
from pathlib import Path
import subprocess
import sys


def main(window_id: str, gui_pid: int) -> None:
    if sys.platform != "linux" or os.geteuid() == 0:
        raise AssertionError("requires unprivileged Linux/Xvfb desktop caller")
    if not os.environ.get("DISPLAY"):
        raise AssertionError("no disposable X11 display")
    if gui_pid <= 1 or gui_pid == os.getpid():
        raise AssertionError("must target separate real GUI process")
    if not window_id.isdecimal() or int(window_id) <= 0:
        raise AssertionError("window ID must identify an existing test window")

    status = Path(f"/proc/{gui_pid}/status").read_text(encoding="utf-8")
    fields = dict(
        line.split(":", 1) for line in status.splitlines() if ":" in line
    )
    gui_uid = int(fields["Uid"].split()[1])
    if gui_uid != os.geteuid():
        raise AssertionError("test requires attacker and GUI under same desktop UID")
    effective_caps = int(fields["CapEff"].strip(), 16)
    if effective_caps:
        raise AssertionError("test GUI unexpectedly holds privileged capabilities")
    own_caps = int(
        next(line for line in Path("/proc/self/status").read_text().splitlines()
             if line.startswith("CapEff:")).split(":", 1)[1].strip(), 16
    )
    if own_caps:
        raise AssertionError("untrusted desktop attacker holds capabilities")

    # Target exactly the already identified disposable Xvfb GUI window.
    # No knowledge of the Riesel app's RPC address or any Manager API.
    for operation in (
        ["xdotool", "windowfocus", "--sync", window_id],
        ["xdotool", "key", "--clearmodifiers", "ctrl+i"],
    ):
        result = subprocess.run(
            operation,
            capture_output=True,
            text=True,
            cwd="/tmp",
            timeout=7,
            check=False,
        )
        if result.returncode:
            raise AssertionError(
                f"X11 input injection setup failed: {result.stderr[-500:]}"
            )
    print("RESEARCH_M59_SAME_UID_X11_CLIENT_INJECTED_GUI_INCREMENT", flush=True)


if __name__ == "__main__":
    if len(sys.argv) != 3:
        raise SystemExit("usage: attacker.py <disposable_x11_window> <gui_pid>")
    main(sys.argv[1], int(sys.argv[2]))
