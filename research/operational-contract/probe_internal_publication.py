#!/usr/bin/env python3
"""#157: genuine SDK Counter whole-session recovery in one OS process.

Both the existing synchronous app-facing test adapter and the new Riesel-owned
async worker use a fresh real managed crash fixture and recovery policy.
This is not an external app exporter, principal verifier or production auth.
"""
from __future__ import annotations

import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
BIN = ROOT / "target" / "debug"


def verify_scenario(binary_name: str, required_markers: tuple[str, ...]) -> None:
    # Separate marker per independent scenario: *both* must experience
    # an actual crash before the replacement managed child becomes healthy.
    with tempfile.TemporaryDirectory(prefix=f"riesel-i157-{binary_name}-") as temp:
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
            [str(BIN / binary_name), "--deployment", str(deployment)],
            cwd=ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            check=False,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"{binary_name} returned {result.returncode}\n"
                f"stdout: {result.stdout}\nstderr: {result.stderr}"
            )
        for marker_text in required_markers:
            if marker_text not in result.stdout:
                raise AssertionError(
                    f"{binary_name}: missing {marker_text!r}\n{result.stdout}"
                )
        if not marker.exists():
            raise AssertionError(
                f"{binary_name}: actual managed Counter crash was never exercised"
            )
        print(f"RESEARCH_I157_SCENARIO_PASS {binary_name}")


def main() -> None:
    verify_scenario(
        "counter_recovery_client",
        (
            "RIESEL_RECOVERY_OK",
            "INTERNAL_PUBLICATION_RECOVERY_OK",
            "generation_changed=true",
            "old_effect_rejected=true",
        ),
    )
    verify_scenario(
        "counter_async_publication_recovery",
        (
            "INTERNAL_ASYNC_MANAGED_RECOVERY_OK",
            "generation_changed=true",
            "owned_worker_cancelled=true",
            "stale_effect_rejected=true",
            "adapter_withdrawn=true",
        ),
    )
    print("RESEARCH_I157_REAL_COUNTER_RUNTIME_INGRESS_PASS")


if __name__ == "__main__":
    main()
