#!/usr/bin/env python3
"""Reproducible *source* comparisons; never interpret LOC as risk or quality."""
from pathlib import Path
import re

root = Path(__file__).resolve().parents[2]
paths = {
    "Counter generated facade (app process)": "examples/counter/app/tests/fixtures/counter_public_facade_host.rs",
    "Counter resolved manual facade": "examples/counter/app/tests/fixtures/counter_resolved_facade_host.rs",
    "Shared preexisting listener helper": "examples/counter/app/tests/fixtures/facade_host_common.rs",
    "Todo HTTP routing": "examples/todo/src/http/routes.rs",
    "Todo HTTP server/worker": "examples/todo/src/http/mod.rs",
    "Reusable research exporter policy": "research/application-export/src/lib.rs",
    "Two-application research adapters/tests": "research/application-export/tests/reuse.rs",
}

def count(src: str) -> int:
    # Count nonblank non-comment lines; explicit only a crude repetition proxy.
    return sum(bool(s.strip()) and not s.lstrip().startswith(("//", "#!")) for s in src.splitlines())

for name, path in paths.items():
    src = (root / path).read_text(encoding="utf-8")
    print(f"{count(src):4} nonblank/non-comment LOC | {name} | {path}")

counter = (root / paths["Counter resolved manual facade"]).read_text()
fixture = (root / paths["Two-application research adapters/tests"]).read_text()
routes = (root / paths["Todo HTTP routing"]).read_text()
arm_pattern = r'"(?:value|add|label)"\\s*=>'
route_pattern = r'pub\\(super\\) async fn (?:list|show|create|patch|delete)\\('
manual_arms = len(re.findall(arm_pattern, counter))
prototype_arms = len(re.findall(arm_pattern, fixture))
route_functions = len(re.findall(route_pattern, routes))
print(f"manual SDK-resolved Counter dispatch arms: {manual_arms} / 3 operations")
print(f"prototype Counter relay dispatch arms: {prototype_arms} / 3 operations")
print(f"Todo authored HTTP route functions: {route_functions} / 5 routes")
print("Interpretation: app-specific behavior/routes are not semantically equivalent to generated RPC operations.")
