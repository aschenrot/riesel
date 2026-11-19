# Riesel delivery model

**Status:** lightweight working agreement, not adoption of the full PM² or PRINCE2 7 methodologies.

## Purpose

Maintain a clear, auditable path from a product outcome to accepted implementation without producing duplicate plans or status reports. Project management must support decisions and testing rather than add ceremony.

## Sources of truth

| Question | Owner |
| --- | --- |
| Why does a feature exist, and what product outcome is accepted? | Product definition |
| What must be investigated or specified before a decision? | GitHub specification Issue |
| What technical semantics must remain true? | Architecture contract or accepted decision |
| What are the delivery stages and acceptance gates? | Implementation roadmap and GitHub Milestones, when configured |
| What accepted work needs implementation? | GitHub implementation Issue |
| What changed and why was it accepted? | Pull Request |
| What was verified at a specific revision? | CI, tests, and commit SHA |
| What happened historically? | Git history |

Do not duplicate open-issue status or past commit histories in the roadmap. Exploratory product directions are not automatically implementation backlog items. A bounded investigation may be accepted and tracked before its proposed implementation is accepted.

## Requirements-to-delivery workflow

1. **Identify the need.** State the intended user or platform outcome, its owner, and known constraints. Keep uncommitted ideas exploratory; an accepted product requirement belongs in the relevant product definition.
2. **Investigate and specify when needed.** Open a specification Issue for a bounded question whose answer requires independent analysis, comparison, or validation. Establish functional behavior, material quality attributes (such as durability, security, availability and compatibility), failure cases, alternatives, risks, dependencies, and testable acceptance boundaries. Spikes may produce evidence, but do not establish production guarantees.
3. **Accept, defer, or reject.** Record an explicit decision by the product owner or maintainer in the specification Issue or a reviewed PR. Update the product definition for accepted outcomes and an existing architecture contract (or concise decision record when warranted) for durable technical invariants. Link the decision and its rationale; a green CI result alone does not accept a requirement. Close the specification Issue with its decision, including when rejected or deferred.
4. **Plan implementation.** Create implementation Issues for independently verifiable outcomes grounded in accepted scope, with dependencies and test evidence. Split work when boundaries, ownership, or verification genuinely differ; not for each checklist item. Implementation can proceed in parallel only where the relevant requirements and interfaces are already accepted.
5. **Implement and verify.** Use coherent PRs, review against the accepted specification, and close implementation Issues only when their acceptance criteria are supported by the tested revision. If findings invalidate a significant requirement or guarantee, revisit the decision before silently expanding scope.

### When a separate specification gate is warranted

Use a separate specification Issue and explicit acceptance before implementing a new or materially changed security, authorization, persistent-data/durability, remote-transport, recovery/lifecycle, published-contract, or cross-cutting architectural guarantee. Also use it when alternatives or acceptance conditions are genuinely unresolved.

For small, low-risk, well-understood changes, record the requirement, decision, and acceptance criteria directly in one implementation Issue and PR. A separate specification Issue, architecture document, meeting, or approval ceremony is **not** mandatory.

A specification is **ready for implementation** once the intended behavior, key invariants, failure/compatibility boundaries, relevant alternatives and exclusions, dependencies, and observable acceptance criteria are clear enough to review. The gate approves scope and guarantees, not necessarily every implementation detail. Record an accountable approval or documented solo-maintainer decision; do not infer approval from an Issue's existence.

## GitHub Issue contract

Use the existing Issue forms for two work types:

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

A specification Issue is not a parent Issue by default, and implementation Issues do not require native sub-issue relationships or a Projects board. Use ordinary Issue links for dependencies. Do not create a separate project charter, business case, or risk log for each feature.

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
