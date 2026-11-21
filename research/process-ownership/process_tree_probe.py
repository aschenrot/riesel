#!/usr/bin/env python3
"""Research-only Linux/POSIX process ownership probe; requires Python stdlib.

Does not test Riesel process supervision or systemd/Windows containment.
Every surviving test child is explicitly killed in finally.
"""
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import time


def pid_state(pid):
    try:
        return pathlib.Path(f"/proc/{pid}/stat").read_text().split(') ', 1)[1][0]
    except FileNotFoundError:
        return None


def wait_not_running(pid, seconds=3):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        if pid_state(pid) in (None, 'Z', 'X'):
            return True
        time.sleep(.02)
    return False


def main():
    if sys.platform != 'linux':
        print('SKIP: Linux /proc semantics required')
        return 0
    with tempfile.TemporaryDirectory() as tmp:
        marker = pathlib.Path(tmp) / 'descendant.pid'
        # Starts a child and exits abnormally, without running Python cleanup.
        launcher = """import os, subprocess, sys
p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(20)'],start_new_session=True)
with open(sys.argv[1],'w') as f:f.write(str(p.pid))
os._exit(0)
"""
        subprocess.run([sys.executable, '-c', launcher, str(marker)], check=True, timeout=4)
        pid = int(marker.read_text())
        try:
            survives_parent = pid_state(pid) not in (None, 'Z', 'X')
            print(f'parent_exit_leaves_child_running={survives_parent}')
            assert survives_parent
            # This descendant started a new session so its PGID equals PID.
            os.killpg(pid, signal.SIGKILL)
            contained = wait_not_running(pid)
            print(f'explicit_process_group_kill_stopped_child={contained}')
            assert contained
        finally:
            try:
                os.killpg(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
    print('LIMIT: not a tree-containment guarantee (descendants may leave group); no systemd or Riesel integration')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
