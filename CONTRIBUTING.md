# Contributing to Riesel

Riesel is an experimental Rust application platform. Breaking API changes are permitted during this architecture phase; a passing test suite does not imply production readiness.

## Where information belongs

- **Product definitions** (`docs/product/*`): intended outcomes, scope, and durable requirements.
- **Architecture contracts** (`docs/product/riesel/*contract*.md` and API documents): accepted technical semantics and invariants.
- **Implementation roadmap** (`docs/product/riesel/implementation-roadmap.md`): active stages, dependencies, and acceptance gates; not a commit journal.
- **GitHub Issues**: actionable work, risks, dependencies, and acceptance criteria.
- **Pull Requests and CI**: change review and evidence that a particular revision was verified.
- **Git history**: implementation chronology. Record lasting, non-obvious architecture decisions separately only when necessary.

See [delivery model](docs/project/delivery.md) for the lightweight PM²/PRINCE2-inspired management rules.

## Working on a change

1. Use or create an Issue for a meaningful engineering outcome. A separate Issue is optional for a trivial typo or mechanical cleanup.
2. Create a short-lived branch from `main`. Prefer `feat/`, `fix/`, `docs/`, or `chore/` followed by a specific topic.
3. Keep the change coherent: implementation, tests, and relevant documentation can belong in the same PR. Split unrelated outcomes.
4. Open a PR targeting `main`, link the Issue, and describe results, verification, and limitations. A draft PR is appropriate while the base or work is not ready.
5. Merge only after required CI checks pass on the intended result and acceptance criteria are satisfied. Prefer squash merges for ordinary feature PRs; keep meaningful full-history merges when justified.
6. Delete merged short-lived branches. Do not force-push a shared branch or modify another active workstream without coordination.

**Integration branch:** `main` is the default and authoritative development baseline. Start new work from its latest verified revision; open feature PRs against `main`. Do not develop on the retired `feature/schema_based_app` branch.

## Validation

From the workspace root:

```sh
cargo validate --check
cargo smoke
```

Linux validation and executable smoke tests are the implementation acceptance baseline. A failed or in-progress check is not a successful gate. Do not promote an unverified development commit to `main`. Windows validation remains an explicit manual pre-release gate while it is not required for routine development.

Review the affected error semantics, session ownership, lifetime, cancellation, state durability, and trust boundaries when a change touches them. Never infer transport authentication or persistent storage from passing reference tests.

## Review and release discipline

For solo development, a PR and green CI provide useful change isolation without requiring artificial approvals. Confirm the exact tested SHA and review the diff before merging. Record major design trade-offs in a durable architecture decision when they would otherwise be lost; routine implementation detail belongs in the PR.

A stage is complete only when its accepted outcomes and tests pass. A release requires its own explicit readiness criteria; completion of a development stage is not a production-readiness claim.
