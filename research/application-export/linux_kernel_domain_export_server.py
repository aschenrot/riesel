#!/usr/bin/env python3
"""#60/#75 research ONLY: kernel AppArmor peer label on an actual Riesel Counter.

Reuses the unmodified PR #149 OS-isolated gateway + genuine PR #90 Counter.
The only change is a *research-only* mandatory pre-dispatch check of Linux
SO_PEERSEC in front of PR #149's UID-scoped operation grant.

No production client authorization, portable IPC, Manager or trust issuance.
"""
from __future__ import annotations

import importlib.util
import os
from pathlib import Path
import socket
import sys

BASE = Path(__file__).resolve().with_name("linux_isolated_export_server.py")


def main():
    if len(sys.argv) != 5 or sys.argv[1] != "--serve":
        raise SystemExit("usage: label_export_server.py --serve <install> <runtime> <profile>")
    if os.geteuid() != 0:
        raise AssertionError("only isolated root TEST gateway may publish app")
    profile = sys.argv[4]
    if not profile.startswith("riesel_m60_client_") or len(profile) > 90:
        raise AssertionError("untrusted research profile name")
    if not BASE.is_file():
        raise AssertionError("must install pinned, unchanged PR #149 gateway")

    spec = importlib.util.spec_from_file_location("riesel_m60_pinned_gateway", BASE)
    if spec is None or spec.loader is None:
        raise AssertionError("cannot load pinned gateway")
    import sys as runtime_sys
    module = importlib.util.module_from_spec(spec)
    runtime_sys.modules[spec.name] = module
    spec.loader.exec_module(module)

    original_peer = module.peer
    original_gateway = module.Gateway

    def real_labeled_peer(connection):
        identity = original_peer(connection)
        if not hasattr(socket, "SO_PEERSEC"):
            raise AssertionError("actual kernel SO_PEERSEC is required")
        # The security label is kernel supplied, not JSON or user-side text.
        label = connection.getsockopt(
            socket.SOL_SOCKET, socket.SO_PEERSEC, 1024
        ).rstrip(b"\0").decode("utf-8", "strict")
        identity["kernel_security_label"] = label
        return identity

    class ResearchLabeledGateway(original_gateway):
        def client(self, request, credential):
            if request.get("operation") == "descriptor":
                # Public interface metadata is not a security token.
                return super().client(request, credential)
            label = credential["kernel_security_label"]
            # AppArmor may append '(enforce)' to the label; require the
            # *entire* prefix to be the exact expected loaded profile.
            if label != profile and label != profile + " (enforce)":
                return {
                    "error": "untrusted_kernel_client_domain",
                    "observed_uid": credential["uid"],
                    "observed_pid": credential["pid"],
                    "observed_label": label,
                }
            # The previously verified real gateway still enforces UID
            # operation grants, epoch and bounded real Counter dispatch.
            return super().client(request, credential)

    module.peer = real_labeled_peer
    module.Gateway = ResearchLabeledGateway
    module.serve(sys.argv[2], sys.argv[3])


if __name__ == "__main__":
    main()
