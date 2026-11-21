# Cross-product research integration preview

**Status:** review-only executable test composition, **not** a merged integration branch, runtime feature, public contract, security approval or implementation authorization. Exact source revisions are pinned in [heads.json](heads.json). [assemble.py](assemble.py) verifies each changed-file list, explicitly reconciles known overlaps and constructs a **disposable working tree** on the CI runner. It never creates a commit, moves a branch or merges a PR.

## Sources and correctness obligations

The preview combines draft research evidence from [#84](https://github.com/aschenrot/riesel/pull/84) (scheduler/#83), [#86](https://github.com/aschenrot/riesel/pull/86) (accepted direction, pending documentary review), [#87](https://github.com/aschenrot/riesel/pull/87) (negative stale/reply cases), [#88](https://github.com/aschenrot/riesel/pull/88) (developer tooling), [#89](https://github.com/aschenrot/riesel/pull/89) (Manager), [#90](https://github.com/aschenrot/riesel/pull/90) (native GUI), [#91](https://github.com/aschenrot/riesel/pull/91) (reusable exporter) and [#92](https://github.com/aschenrot/riesel/pull/92) (operational attachment).

The source set has **only two recorded path overlaps**:

1. `.github/workflows/validate.yml` changes from #84, #91 and #92: keep the original normal CI workflow unmodified. Their *research-only* checks instead run in a dedicated [integration-research.yml](../../.github/workflows/integration-research.yml) on this preview. Do not silently make experimental Bevy, RunenECS, frontend, GUI or Python dependencies mandatory for other PRs.
2. `docs/product/riesel/application-operational-boundary-investigation.md` from #87 and #92: retain the #92 top-of-document S1/S2 cross-reference **and** append #87's distinct negative-evidence section. Verify the original #87 appendix still matches the exact main baseline and refuse unknown document layouts.

Any new overlap or changed source file list fails immediately instead of silently choosing the last writer. Each source branch's exact SHA/base SHA and file set are treated as immutable; changing one requires explicit reinspection and manifest update. #84 was branched from an older main revision; apply only its reviewed file-level delta, not a replacement of newer product documents.

The assembled SHA printed as `RESEARCH_INTEGRATION_TREE_SHA` is a Git **tree**, not a commit on `main`. It is reproducible from this draft integration revision and pinned source commits. Individual prior CI passes do not substitute for this combined test run.

## Verification matrix for this preview

| CI job | Combined evidence | Limits |
| --- | --- | --- |
| `combined-core` | Actual merged-content `cargo validate --check`, `cargo smoke` (including #87), #59 direct owner probes, #75 real process and same-PID probes, #91 real Counter/Todo policy comparator, #83 real RunenECS/Bevy tests | **No actual systemd owner**, no security-safe same-UID authorization, no runtime-issued public session epoch, no production exporter, no complete scheduler/plan authoring |
| `combined-developer` | Independent Cargo and Todo persistent browser lifecycle, real static assets and Chromium, existing browser E2E tests | No shipped reusable launcher, generic graceful app stop, portable process-tree containment or authenticated readiness |
| `combined-native` | Actual Linux Xvfb independent GUI event loop and backend lifecycle | No permanent toolkit, screen-reader certification, other OS GUI proof or secure external attachment |

A passing combined preview demonstrates that the research content can coexist and execute under the tested Linux environment; it does not upgrade the provisional mocks or claim production correctness. Windows remains a manual pre-release gate. Large release-profile ECS measurements are intentionally not rerun on every integration preview, as they are noisy host-specific comparative observations rather than correctness gates; #84's pinned original run remains its measurement record. #83's isolated research lockfile is not yet committed; this is a **known reproducibility gap** requiring a separately reviewed fix.

## Decisions deliberately left open

- **#75:** authoritative app/runtime-issued session generation with revocation/dispatch race proof; server and caller authentication compatible with the actual same-UID threat profile; authenticated callable readiness and unknown mutation outcomes.
- **#68:** optional Cargo tool and SDK participation, how app-wide stop is represented, trusted executable inspection, data-root default and explicit UI-build adapters.
- **#59:** test proposed Linux systemd user `.service` owner and exact process fate on an actual user manager; distinguish trusted same-UID operator assumptions from #75 privileged external attachments. Manager command retention/authorization and browser mutations remain separate decisions.
- **#60:** generated forwarding of typed resolved handles, full revision-aware Todo export, authorized attach, toolkit choice and accessibility checks for the first reference GUI.
- **#83:** independently decide headless executable graph composition, local safe-point control, visualization and editable round-trip authoring. Defer universal scheduler/ECS choice. A common operation/attempt identifier is not an app-session or OS-process identifier.

**No source PR is merged or closed by this preview.** After review, integrate durable research documentation and carefully scoped tests into appropriately owned PRs; avoid automatically landing redundant research harnesses or research-only CI in production. Update authoritative product/architecture documents and create implementation issues only after their explicit technical specification decisions, per [delivery workflow](../../docs/project/delivery.md).
