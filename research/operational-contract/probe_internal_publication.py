#!/usr/bin/env python3
"""#157: real SDK-resolved Counter whole-session recovery in one OS process.

Research/test harness only. It uses the existing managed crash fixture and
Riesel-owned recovery policy; no public exporter, identity verifier or auth.
"""
from __future__ import annotations

import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = ROOT / "target" / "debug"


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="riesel-i157-") as temp:
        test_dir = pathlib.Path(temp)
        marker = test_dir / "first-crash.marker"
        deployment = test_dir / "deployment.json"
        deployment.write_text(
            json.dumps(
                {
                    "services": {
                        "counter": {
                            "transport": "managed",
                            "program": str(BIN / "counter_crash_fixture"),
                            "args": [str(marker)],
                            "startup_timeout_ms": 10_000,
                            "shutdown_timeout_ms": 2_000,
                        }
                    },
                    "recovery": {
                        "mode": "restart_application",
                        "max_restarts": 1,
                        "initial_backoff_ms": 25,
                        "max_backoff_ms": 100,
                    },
                },
                indent=2,
            ),
            encoding="utf-8",
        )
        result = subprocess.run(
            [str(BIN / "counter_recovery_client"), "--deployment", str(deployment)],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"recovery binary returned {result.returncode}\n"
                f"stdout: {result.stdout}\nstderr: {result.stderr}"
            )
        for marker_text in (
            "RIESEL_RECOVERY_OK",
            "INTERNAL_PUBLICATION_RECOVERY_OK",
            "generation_changed=true",
            "old_effect_rejected=true",
        ):
            if marker_text not in result.stdout:
                raise AssertionError(
                    f"missing required evidence: {marker_text}\n{result.stdout}"
                )
        if not marker.exists():
            raise AssertionError("managed Counter crash/restart was not exercised")
        print("RESEARCH_I157_REAL_COUNTER_RUNTIME_INGRESS_PASS")


if __name__ == "__main__":
    main()
