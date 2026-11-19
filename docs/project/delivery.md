# Riesel delivery model

**Status:** lightweight working agreement, not adoption of the full PM² or PRINCE2 7 methodologies.

## Purpose

Maintain a clear, auditable path from a product outcome to accepted implementation without producing duplicate plans or status reports. Project management must support decisions and testing rather than add ceremony.

## Sources of truth

| Question | Owner |
| --- | --- |
| Why does a feature exist, and what product outcome is accepted? | Product definition |
| Where are unresolved requirements or architecture questions tracked? | GitHub specification Issue, when needed |
| What technical semantics must remain true? | Architecture contract or accepted decision |
| What are the delivery stages and acceptance gates? | Implementation roadmap and GitHub Milestones, when configured |
| What accepted work needs implementation? | GitHub implementation Issue |
| What changed and why was it accepted? | Pull Request |
| What was verified at a specific revision? | CI, tests, and commit SHA |
| What happened historically? | Git history |

Do not duplicate open-issue status or past commit histories in the roadmap. Exploratory product directions are not automatically implementation backlog items. A bounded investigation may be accepted and tracked before its proposed implementation is accepted.

## Requirements-to-delivery workflow

1. **Identify the need.** State the intended user or platform outcome, its owner, and known constraints. Keep uncommitted ideas exploratory; an accepted product requirement belongs in the relevant product definition.
2. **Investigate and specify when needed.** Track material unresolved questions in a bounded specification Issue. Evaluate relevant behavior, quality attributes, failure cases, alternatives, risks, dependencies, and acceptance criteria. A technical spike may provide evidence but does not establish production guarantees.
3. **Accept, defer, or reject.** Record the product owner or maintainer's decision, rationale, and evidence in a final Issue comment or reviewed PR. Update the product definition for accepted outcomes and an existing architecture contract (or concise decision record when warranted) for durable invariants. Close the specification Issue with its recorded outcome; acceptance of a specification does not itself approve or deliver implementation. CI alone is not a requirements decision.
4. **Plan implementation.** Create implementation Issues for independently verifiable outcomes grounded in accepted scope, with dependencies and test evidence. Split work when boundaries, ownership, or verification genuinely differ; not for each checklist item. Implementation can proceed in parallel only where the relevant requirements and interfaces are already accepted.
5. **Implement and verify.** Use coherent PRs, review against the accepted specification, and close implementation Issues only when their acceptance criteria are supported by the tested revision. If findings invalidate a significant requirement or guarantee, revisit the decision before silently expanding scope.

### When a separate specification gate is warranted

Use a separate specification Issue when material decisions remain unresolved, especially about security, durable data, remote transport, recovery, published contracts, or cross-cutting architecture. A previously accepted and linked specification is sufficient; the feature category alone does not mandate another Issue.

Small or well-understood changes can record their acceptance criteria directly in one implementation Issue and PR. Urgent corrective work may proceed without a separate investigation when delay would increase risk; document the assumptions, verification, and any unresolved follow-up. No separate meeting or document is required by default.

Implementation is ready when the relevant behavior, guarantees, failure/compatibility boundaries, exclusions, dependencies, and verifiable outcomes are clear and an accountable maintainer has recorded acceptance. This does not require deciding every implementation detail.

### Evidence-based investigation (when alternatives matter)

A specification Issue should state the decision, requirements and evidence needed **before** a solution is favored. For a material choice among alternatives, document candidate discovery scope, eligibility criteria, comparable evaluation dimensions and evidence sources; separate mandatory guarantees from preferences. Record dated findings, unknowns and exclusion reasons, and use focused tests only for decision-critical gaps. The outcome may be a recommendation **or a documented deferral**; neither a candidate inventory nor passing CI constitutes acceptance. This is part of the existing specification workflow, not a new mandatory Issue type or research ceremony.

## GitHub Issue contract

For planned work, use two Issue forms. Bug reports retain their existing separate form:

| Type | Deliverable | Closure |
| --- | --- | --- |
| **Specification / investigation** | Evaluated options and a recorded accept/defer/reject decision, with accepted requirements or architecture updated where applicable | Decision and evidence are linked; no implementation is implied |
| **Implementation** | A bounded change meeting accepted requirements | Tests, PR and applicable acceptance evidence are linked |

For either type record only:
1. **Outcome / question:** the expected benefit or decision.
2. **Scope:** intended deliverables and important exclusions.
3. **Acceptance:** observable evidence needed to close this Issue.
4. **Dependencies / exceptions:** only material blockers, risks, or decisions.
5. **Evidence:** relevant PR, tests, and linked authoritative documents.

Use ordinary Issue links for dependencies; a parent/sub-issue hierarchy or project board is optional. Do not create a separate project charter, business case, or risk log for each feature.

## Stage management

Use a GitHub Milestone for an accepted delivery stage when milestone management is available. A stage has a stated outcome, bounded scope, verifiable exit criteria, and linked Issues. The roadmap contains a compact cross-stage view; it is not a running engineering diary.

At a stage boundary, check:
- Are the acceptance criteria satisfied by the tested revision?
- Is CI green for that exact revision and are release-specific checks distinguished?
- Are important open risks, exclusions, or deviations recorded?
- Is the next stage justified, or should scope/order change?

Keep a decision record only when a durable architectural trade-off would otherwise be hard to recover. PR discussions are sufficient for ordinary implementation choices.

## Management by exception

Routine implementation changes are handled inside Issues and PRs. Explicitly reassess a decision or stage only if:
- Validation fails or evidence is missing at its acceptance gate.
- A security, data durability, compatibility, or recovery guarantee changes.
- Required scope, assumptions, or dependencies change materially.
- A non-cooperative task, unsafe retry, or unavailable infrastructure would invalidate the intended outcome.

Riesel currently permits aggressive API cleanup, but breaking changes still require accurate tests, examples, and documentation. Do not claim production readiness from experimental reference components.

## Integration policy

Use short-lived branches created from `main` and target `main` in coherent Pull Requests. Merge only after relevant Linux validation and smoke checks pass on the intended result; favor squash merges for routine work and preserve full history where justified. Branch protection and default-branch settings are repository controls, not substitutes for CI evidence or code review.

Linux `cargo validate --check` and `cargo smoke` are the normal acceptance checks. Windows remains an explicit manual pre-release check until policy changes through an accepted PR.
