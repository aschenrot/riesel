# Riesel delivery model

**Status:** lightweight working agreement, not adoption of the full PM² or PRINCE2 7 methodologies.

## Purpose

Maintain a clear, auditable path from a product outcome to accepted implementation without producing duplicate plans or status reports. Project management must support decisions and testing rather than add ceremony.

## Sources of truth

| Question | Owner |
| --- | --- |
| Why does a feature exist? | Product definition |
| What semantics must remain true? | Architecture contract or accepted decision |
| What are the delivery stages and acceptance gates? | Implementation roadmap and GitHub Milestones, when configured |
| What needs to change next? | GitHub Issue |
| What changed and why was it accepted? | Pull Request |
| What was verified at a specific revision? | CI, tests, and commit SHA |
| What happened historically? | Git history |

Do not duplicate open-issue status or past commit histories in the roadmap. An Item belongs to an Issue only after its scope has been accepted for implementation; exploratory product directions are not automatically backlog items.

## PM²-lite issue contract

For a meaningful change, record only:
1. **Outcome:** the expected benefit or corrected behavior.
2. **Scope:** intended deliverables and important exclusions.
3. **Acceptance:** observable results and validation evidence.
4. **Dependencies / exceptions:** only material blockers, risks, or decisions.
5. **Evidence:** relevant PR, tests, and linked documents.

Issue forms provide this template. Do not create a separate project charter, business case, or risk log for each feature.

## Stage management

Use a GitHub Milestone for an accepted delivery stage when milestone management is available. A stage has a stated outcome, bounded scope, verifiable exit criteria, and linked Issues. The roadmap contains a compact cross-stage view; it is not a running engineering diary.

At a stage boundary, check:
- Are the acceptance criteria satisfied by the tested revision?
- Is CI green for that exact revision and are release-specific checks distinguished?
- Are important open risks, exclusions, or deviations recorded?
- Is the next stage justified, or should scope/order change?

Keep a decision record only when a durable architectural trade-off would otherwise be hard to recover. PR discussions are sufficient for ordinary implementation choices.

## Management by exception

Routine implementation changes are handled inside Issues and PRs. Explicitly reassess a stage only if:
- Validation fails or evidence is missing at its acceptance gate.
- A security, data durability, compatibility, or recovery guarantee changes.
- Required scope or dependencies change materially.
- A non-cooperative task, unsafe retry, or unavailable infrastructure would invalidate the intended outcome.

Riesel currently permits aggressive API cleanup, but breaking changes still require accurate tests, examples, and documentation. Do not claim production readiness from experimental reference components.

## Integration policy

Use short-lived branches created from `main` and target `main` in coherent Pull Requests. Merge only after relevant Linux validation and smoke checks pass on the intended result; favor squash merges for routine work and preserve full history where justified. Branch protection and default-branch settings are repository controls, not substitutes for CI evidence or code review.

Linux `cargo validate --check` and `cargo smoke` are the normal acceptance checks. Windows remains an explicit manual pre-release check until policy changes through an accepted PR.
