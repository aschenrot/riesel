# Riesel delivery model

**Status:** lightweight working agreement, not adoption of the full PM² or PRINCE2 7 methodologies.

## Purpose

Maintain a clear, auditable path from a product outcome to accepted implementation without producing duplicate plans or status reports. Prefer coherent solutions that remain useful as the product grows; avoid both recurring one-off patches and speculative frameworks. Project management must support decisions and testing rather than add ceremony.

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

1. **Identify the need.** State the intended user or platform outcome, its owner, known constraints, and whether its intended reuse is application-specific or platform-wide. Check this scope against the product definition and plausible future uses **before** considering solutions. An example can test the requirement but cannot silently narrow it. Keep uncommitted ideas exploratory; an accepted product requirement belongs in the relevant product definition.
2. **Investigate and specify when needed.** Track material unresolved questions in a bounded specification Issue. Compare alternatives against the **same original product outcome** and an explicit, realistic growth horizon. Evaluate functionality, flexibility, reuse, operational costs, failure cases, security, migration costs and reversibility, not just initial implementation size. A technical spike may provide evidence but does not establish production guarantees.
3. **Accept, defer, or reject.** Before acceptance, ask whether satisfying *all* proposed tests would fulfill the original product outcome across its intended users and reuse boundary. Record the product owner or maintainer's decision, rationale, and evidence in a final Issue comment or reviewed PR. Update the product definition for accepted outcomes and an existing architecture contract (or concise decision record when warranted) for durable invariants. Close the specification Issue with its recorded outcome; acceptance of a specification does not itself approve or deliver implementation. CI alone is not a requirements decision.
4. **Plan implementation.** Create implementation Issues for independently verifiable outcomes grounded in accepted scope, with dependencies and test evidence. Split work when boundaries, ownership, or verification genuinely differ; not for each checklist item. Implementation can proceed in parallel only where the relevant requirements and interfaces are already accepted.
5. **Implement and verify.** Use coherent PRs, review against the accepted specification, and close implementation Issues only when their acceptance criteria are supported by the tested revision. If findings invalidate a significant requirement or guarantee, revisit the decision before silently expanding scope.

### Strategic fit and bias control

**Intentional preference:** favor a coherent, reusable and evolvable design when the accepted product outcome spans applications, teams or likely future integrations. Choose the **smallest coherent architecture**, not automatically the smallest patch or the most general framework. A bounded first increment may leave capabilities for later, but should not knowingly impose avoidable reimplementation, contract migration or duplicated infrastructure to reach the accepted outcome.

For material design choices, challenge both directions:

- **Short-term bias:** existing-code preference, lowest initial effort, artificially narrow reference-example scope, and deferring a product requirement until several copies of the same workaround exist. Compare the cumulative cost of repetition, integration and migration.
- **Overengineering bias:** presumed need for new crates, commands, layers, standards or universal abstractions based only on speculative future features. Require a specific use case, boundary or risk that the added complexity actually addresses.
- **Architecture durability:** identify likely extension points, ownership boundaries, stable public contracts, deployment/platform variants, operational failure modes and how an accepted first increment can evolve without redesign. Consider maintenance and switching costs over a stated, plausible horizon rather than forecasting arbitrary features.

The product requirement establishes **what outcome is worth supporting**; research determines **which mechanism is justified**. Multiple examples can validate an architecture but are not inherently a prerequisite to recognizing a platform-level requirement. Record decisive evidence, unknowns and trade-offs only where they change the decision. No scoring formula, extra approval stage, architecture diagram or new document is mandatory.

### When a separate specification gate is warranted

Use a separate specification Issue when material decisions remain unresolved, especially about security, durable data, remote transport, recovery, published contracts, or cross-cutting architecture. A previously accepted and linked specification is sufficient; the feature category alone does not mandate another Issue.

Small or well-understood changes can record their acceptance criteria directly in one implementation Issue and PR. Urgent corrective work may proceed without a separate investigation when delay would increase risk; document the assumptions, verification, and any unresolved follow-up. No separate meeting or document is required by default.

Implementation is ready when the relevant behavior, guarantees, failure/compatibility boundaries, intended reuse and evolution boundaries, exclusions, dependencies, and verifiable outcomes are clear and an accountable maintainer has recorded acceptance. This does not require deciding every implementation detail.

### Evidence-based investigation (when alternatives matter)

For material decisions among alternatives, define a bounded research plan: original product outcome and reuse scope, a realistic growth horizon, required outcomes versus preferences, common comparison criteria and evidence sources. Consider both reuse of existing mechanisms and a coherent shared capability when relevant; justify exclusions rather than assuming either is preferable. Record dated findings, unknowns and exclusion reasons. Investigate decision-critical gaps with focused tests; stop when more research is unlikely to change the choice **for the full accepted outcome**, or defer when evidence is insufficient. A reviewed research plan establishes scope; preliminary discovery needs no approval, and the plan does **not** approve the resulting architecture or implementation. Use the existing specification Issue rather than requiring another Issue type.

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
