#!/usr/bin/env python3
"""#75 real Riesel same-PID recovery fixture, isolated from production APIs.

Run after cargo smoke; requires Counter example's research host and crash binary.
This checks a per-session app-handler-issued epoch, NOT a runtime public exporter
or a hostile-same-UID authentication design.
"""
import json
import os
import queue
import subprocess
import tempfile
import threading
import time
from pathlib import Path

from probe import exchange, message


def main():
    root = Path(__file__).resolve().parents[2]
    executable = '.exe' if os.name == 'nt' else ''
    host = root / 'target' / 'debug' / ('counter_same_pid_recovery_host' + executable)
    crashing_provider = root / 'target' / 'debug' / ('counter_crash_fixture' + executable)
    for binary in (host, crashing_provider):
        if not binary.is_file():
            raise RuntimeError(f'required real Riesel executable missing: {binary}; run cargo smoke')

    with tempfile.TemporaryDirectory(prefix='riesel-75-same-pid-') as folder:
        temporary = Path(folder)
        marker = temporary / 'first-provider-exited'
        config = temporary / 'deployment.json'
        config.write_text(json.dumps({
            'services': {'counter': {
                'transport': 'managed',
                'program': str(crashing_provider),
                'args': [str(marker)],
                'startup_timeout_ms': 10000,
                'shutdown_timeout_ms': 2000,
            }},
            'recovery': {
                'mode': 'restart_application',
                'max_restarts': 1,
                'initial_backoff_ms': 25,
                'max_backoff_ms': 100,
            },
        }))
        process = subprocess.Popen([str(host), '--deployment', str(config)],
                                   stdout=subprocess.PIPE, stderr=None, text=True)
        try:
            announcements = queue.Queue(maxsize=1)
            threading.Thread(target=lambda: announcements.put(process.stdout.readline()),
                             daemon=True).start()
            first_line = announcements.get(timeout=20).strip()
            assert first_line.startswith('RESEARCH_SAME_PID_BOUND '), first_line
            endpoint, pid = first_line.removeprefix('RESEARCH_SAME_PID_BOUND ').rsplit(' ', 1)
            hostname, port = endpoint.rsplit(':', 1)
            address = hostname, int(port)
            assert int(pid) == process.pid

            def send(kind, **kwargs):
                return exchange(address, message(kind, **kwargs))

            def phase(want, timeout):
                until = time.monotonic() + timeout
                while time.monotonic() < until:
                    if process.poll() is not None:
                        raise AssertionError(f'app process exited before phase {want}: {process.returncode}')
                    response = send('status')
                    assert response['pid'] == process.pid
                    if response['phase'] == want:
                        return
                    time.sleep(.03)
                raise AssertionError(f'Riesel application did not reach phase {want}')

            def bind():
                return send('attach', proof='research-write-NOT-A-CREDENTIAL')

            def call(binding, operation, arguments=None):
                return send('call', generation=binding['generation'],
                            attachment=binding['attachment'], operation=operation,
                            arguments=arguments or {})

            phase(1, 8)
            old = bind()
            assert call(old, 'value') == {'result': {'Ok': 0}}
            assert call(old, 'add', {'amount': 1}) == {'result': {'Ok': 1}}
            phase(2, 15)  # Managed Counter crashed; Riesel reran entrypoint in this PID.
            assert marker.exists(), 'fault-injected managed provider did not crash'
            assert call(old, 'add', {'amount': 2}) == {'error': 'stale_generation'}
            fresh = bind()
            assert fresh['generation'] != old['generation']
            assert call(fresh, 'value') == {'result': {'Ok': 0}}
            assert call(fresh, 'add', {'amount': 5}) == {'result': {'Ok': 5}}
            assert send('stop', generation=fresh['generation'],
                        attachment=fresh['attachment']) == {'ok': True}
            status = process.wait(timeout=15)
            assert status == 0, f'Riesel recovering host failed: {status}'
            print('RESEARCH_REAL_RIESEL_SAME_PID_RECOVERY_EXTERNAL_STALE_REJECT_PASS')
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)


if __name__ == '__main__':
    main()
