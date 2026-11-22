#!/usr/bin/env python3
"""Verify the research file came from the real bounded Riesel graph run."""
import json
import sys
from uuid import UUID

with open(sys.argv[1], encoding="utf-8") as source:
    snapshot = json.load(source)
assert snapshot["schema"] == "riesel.research.graph-inspection.v1"
assert snapshot["plan_id"] == "example.session-workflow"
assert snapshot["revision"] == 1
assert snapshot["run_sequence"] > 0
assert UUID(snapshot["session_generation"]).version == 4
assert snapshot["phase"] == "Finished"
assert snapshot["session_expired"] is False
nodes = {node["id"]: node for node in snapshot["nodes"]}
assert set(nodes) == {"left", "right", "join"}
assert set(nodes["join"]["depends_on"]) == {"left", "right"}
assert all(node["initial"] == "pending" for node in nodes.values())
assert all(node["final"] == "completed" for node in nodes.values())
assert all(node["attempt"] == 1 for node in nodes.values())
assert snapshot["initial_sequence"] < snapshot["latest_sequence"]
assert snapshot["trace_truncated"] is False
assert snapshot["transitions"]
assert all(
    0 < event["sequence"] <= snapshot["latest_sequence"]
    for event in snapshot["transitions"]
)
print("PASS: real validated plan, actual session/run identity, dependency join, final authoritative states and bounded trace")
