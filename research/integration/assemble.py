#!/usr/bin/env python3
"""Assemble a pinned, disposable integration worktree for research verification.

This does not merge PRs, update refs, publish a runtime API or authorize a
production change. All source revisions and changed paths are explicit.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
MANIFEST = Path(__file__).with_name("heads.json")
VALIDATE = ".github/workflows/validate.yml"
SHARED_DOC = "docs/product/riesel/application-operational-boundary-investigation.md"


def git(*args: str) -> bytes:
    return subprocess.check_output(["git", *args], cwd=ROOT)


def source_file(commit: str, path: str) -> bytes:
    return git("show", f"{commit}:{path}")


def apply_file(head: str, name: str) -> None:
    listing = git("ls-tree", head, "--", name).decode().strip()
    if not listing:
        raise RuntimeError(f"missing source tree entry {head}:{name}")
    metadata, recorded_path = listing.split("\t", 1)
    mode, kind, _ = metadata.split(" ", 2)
    if recorded_path != name or kind != "blob" or mode not in ("100644", "100755"):
        raise RuntimeError(f"unsupported git mode/path for {head}:{name}: {listing}")
    target = ROOT / name
    if target.is_symlink():
        raise RuntimeError(f"refusing to replace a symlink: {name}")
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(source_file(head, name))
    target.chmod(0o755 if mode == "100755" else 0o644)


def main() -> None:
    manifest = json.loads(MANIFEST.read_text(encoding="utf8"))
    if manifest["schema"] != 1:
        raise RuntimeError("unsupported research source manifest")
    base = manifest["main"]
    if len(base) != 40:
        raise RuntimeError("base must be a full commit SHA")
    if not git("cat-file", "-t", base).strip() == b"commit":
        raise RuntimeError("pinned main commit is unavailable")
    sources = manifest["sources"]
    if [item["pr"] for item in sources] != [84, 86, 87, 88, 89, 90, 91, 92]:
        raise RuntimeError("unexpected source PR list; review integration manifest")

    owners: dict[str, list[int]] = {}
    by_pr: dict[int, dict] = {}
    for src in sources:
        n = src["pr"]
        head, ancestor = src["head"], src["base"]
        if len(head) != 40 or len(ancestor) != 40:
            raise RuntimeError(f"unreviewed or abbreviated commit for #{n}")
        for sha in (head, ancestor):
            if subprocess.run(
                ["git", "cat-file", "-e", sha + "^{commit}"],
                cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                check=False,
            ).returncode != 0:
                git("fetch", "--no-tags", "origin", sha)
        actual = set(git("diff", "--name-only", "--diff-filter=ACMRT",
                         ancestor, head).decode().splitlines())
        expected = set(src["files"])
        if actual != expected:
            raise RuntimeError(
                f"#{n} changed file list differs from reviewed source; "
                f"extra={sorted(actual - expected)}, missing={sorted(expected - actual)}"
            )
        by_pr[n] = src
        for name in sorted(expected):
            if name.startswith("/") or ".." in Path(name).parts:
                raise RuntimeError(f"invalid relative source path: {name}")
            owners.setdefault(name, []).append(n)
        print(f"RESEARCH_SOURCE_VERIFIED PR#{n} head={head} files={len(actual)}", flush=True)

    overlaps = {name: nums for name, nums in owners.items() if len(nums) > 1}
    if overlaps != {
        VALIDATE: [84, 91, 92],
        SHARED_DOC: [87, 92],
    }:
        raise RuntimeError(f"unreviewed source overlap: {overlaps}")

    # Keep normal project's existing Validate CI unchanged. Run the isolated
    # research programs in this dedicated CI workflow instead.
    for name, nums in sorted(owners.items()):
        if name in (VALIDATE, SHARED_DOC):
            continue
        apply_file(by_pr[nums[0]]["head"], name)

    # The #92 report adds an S1/S2 reference near the top of the original
    # document, while #87 adds a separate negative-evidence section at EOF.
    # Reconcile both explicitly; never choose a silent last-writer-wins merge.
    baseline = source_file(base, SHARED_DOC)
    prior = source_file(by_pr[87]["head"], SHARED_DOC)
    revised = source_file(by_pr[92]["head"], SHARED_DOC)
    stable = baseline.rstrip(b"\r\n")
    if not prior.startswith(stable):
        raise RuntimeError("negative evidence document no longer appends to baseline")
    appendix = prior[len(stable):].lstrip(b"\r\n")
    if not appendix.startswith(b"## 12. Verified negative two-process lifecycle tests"):
        raise RuntimeError("unrecognized negative evidence section")
    if b"## 12. Verified negative two-process lifecycle tests" in revised:
        raise RuntimeError("section 12 already present in S1/S2 document")
    combined = revised.rstrip(b"\r\n") + b"\n\n" + appendix.rstrip(b"\r\n") + b"\n"
    target = ROOT / SHARED_DOC
    target.write_bytes(combined)
    print("RESEARCH_OVERLAPS_RESOLVED validate.yml=baseline docs=#92-plus-#87", flush=True)

    # Worktree changes are local to the GitHub Actions runner. No git commit,
    # branch update or PR merge is created. The tree SHA is reproducible from
    # this manifest + the integration PR head, subject to Git object mode.
    git("add", "--all")
    tree_sha = git("write-tree").decode().strip()
    print(f"RESEARCH_INTEGRATION_TREE_SHA {tree_sha}", flush=True)
    print(git("diff", "--cached", "--stat").decode(), flush=True)
    print("RESEARCH_INTEGRATION_ASSEMBLY_PASS", flush=True)


if __name__ == "__main__":
    os.chdir(ROOT)
    main()
