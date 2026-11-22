# #111 — Editor-safe authoring, revision authority and publication: targeted research

**Status:** research only; no public API, document format, application publisher, authorization system or owner acceptance of A2/B2/C3. **Scope:** remaining material decisions under [Riesel #111](https://github.com/aschenrot/riesel/issues/111) after owner-accepted source model **C**, editable first subset **S2** with eventual S3 compatibility, and semantic/layout revisions and explicit source transfer **V2**.

**Reproduction baseline:** branch `research/111-authoring-publication-gates` created from `main` `3247b7bd2a0adcc74a9b8daf49933a131c233fcc` (2026-11-22). This research uses the actual `riesel_app` `ExecutionPlanDraft`, `ValidatedExecutionPlan`, `OperationNode`, `OperationKey`, `OperationRegistry`, `OperationTrigger` and `AccessMode` APIs. It **does not** modify production SDK source, introduce public authoring types or merge another active research branch. The test-local ledger/descriptor and serde document are deliberately **not accepted production implementations**.

## 1. Intended outcome and longer-term fit

A Rust-first application author can inspect an executable operation plan, deliberately convert supported plans to document-owned authoring, visually change safe registered S2 control-flow constructs, and publish a new immutable executable revision **without inventing a second runtime**, silently losing code-defined semantics, weakening domain/resource safety, or letting a malicious/stale editor change someone else's app.

Longer term, independently authored workspaces, async services, domain-owned ECS/game frame workloads, multiple authorized inspectors/editors and later S3 typed executable dataflow must remain possible. An inspector and editor have different trust boundaries; headless applications must not require GUI or Manager. This is a cross-product trust/semantic design, not adoption of one UI toolkit or storage engine.

## 2. Verified baseline and evidence limits

Read the current repository delivery contract, product definition, accepted execution-plan and operational contracts, current Rust plan/runner, #111 comments, and research-only [PR #169](https://github.com/aschenrot/riesel/pull/169). Independent Linux CI previously passed #169's five direct-production-plan adapter tests on [run 44985812166](https://github.com/aschenrot/riesel/actions/runs/44985812166). Those tests do not establish multi-writer fencing, OS caller identity, durable plan publication or safe S3 value execution.

**Material baseline gap:** `OperationNode::access` and `domain_tick/delegated_frame` are declarations from the plan author. `ValidatedExecutionPlan::bind` checks only that the exact registered callback is present; it **does not cross-check callback-required write access or native/domain ownership**. An untrusted document that removes the write declaration can still validate and bind, with possible unsafe overlap if executed. The safe editor authoring layer therefore **must** verify compulsory constraints independently of editor-controlled metadata; trusting `OperationRegistry` presence or `ExecutionPlanDescription` JSON alone is insufficient.

`PlanRevisionCatalog` is only a lifetime-local duplicate-revision guard, not durable publisher identity or global CAS. `ExecutionPlanDescription` is Serialize-only read-only. `AsyncOperation` currently returns `OperationOutcome` with no typed values. The delivered [#157](https://github.com/aschenrot/riesel/issues/157) internal runtime-session publication/ingress is not authenticated external editor publication; [#75](https://github.com/aschenrot/riesel/issues/75) has common semantics and product-specific adapters, not an accepted exporter protocol or cross-platform trusted client.

### Existing metadata foundations — reuse versus duplication

The public `riesel_schema::CapabilityContract` already contains versioned, Rust-generated capability operation signatures and input/output type descriptions, exposed via `CapabilityDescriptor::definition()`. `AppSchema` separately describes application requirements, models, linked record-query facets and provider dependencies. These mechanisms demonstrate that the platform can derive **inspectable typed metadata from Rust** instead of maintaining a second hand-written schema.

However, they are **not interchangeable with registered execution-plan operations**: a graph `OperationKey` can identify an arbitrary Rust callback, may capture a resolved capability handle, and has resource/domain/affinity scheduling constraints; it is not a provider selector or the same thing as a capability method's request/response signature. A2 should reuse the existing versioned-definition and Rust-first derivation *pattern*, with explicit references to capability contracts where appropriate, instead of duplicating `AppSchema` or forcing every graph callback into a capability RPC operation. There is no production `OperationDescriptor` or editor-publisher permission catalog today. Reusing a type name without verifying these identity and ownership distinctions would be a migration hazard.

## 3. Alternatives on equal product scope

| Concern | Alternative | Cost / advantage | Failure or future evolution |
| --- | --- | --- | --- |
| **Operation metadata** | A1: infer editor permissions/resources from runtime `OperationRegistry` and read-only graph | Lowest integration cost | No compulsory policy provenance, arbitrary opaque callback restrictions, passive offline editor or independent client permission semantics. **Inadequate**. |
| | A2: inert, application-published versioned editor-safe descriptors with **trusted runtime binding confirmation** | More up-front metadata/registration versioning; passive inspection without executing untrusted app | Keeps S2 control-only and S3 typed ports extensible; requires artifact/catalog mismatch and compulsory constraint checks. **Preferred**. |
| | A3: live runtime-only editable-operation introspection | Fresh dynamic eligibility and strong association with live handler | Requires executing app for inspection and granting remote reader rights, fails passive/offline requirement; may complement A2. |
| **Document compatibility** | B1: permissive JSON/serde deserialize and resave | Easy old editors | Silently strips new S3 executable fields and corrupts meaning. **Inadequate**. |
| | B2: strict executable parse + explicit format/required-feature gate; future docs preserved **read-only**, migration creates new immutable revision | More version/migration diagnostics; no automatic old-client edits | Supports S2-to-S3 non-lossy transition and explicit unknown-feature rejection. **Preferred**. |
| | B3: freeze one universal typed IR/bytecode from day one | Potential theoretical portability | Requires unproven typed ownership, GPU/native affinity, serialization and secure VM semantics before first control-only graph. **Not justified by current requirements**. |
| **Publication authority** | C1: an editor/GUI owns the plan and activates it directly | Quick visible prototype | Duplicates app/session owner, bypasses explicit grants and stale-session fences. **Inadequate**. |
| | C2: universal Manager-owned plan service | Central installed-app ownership | Mandatory Manager for headless independent Cargo apps and conflated OS control with app plan edit permission. **Inadequate as universal path**; optional adapter plausible. |
| | C3: application-authoritative candidate publication + explicit activation under existing session/grants | More explicit provenance, application-scoped receipts and crash reconciliation | Preserves one run owner and #75/#60 adapters; durable store, grant issuer and activation mechanics remain open. **Preferred direction**. |

**Bias check:** A1/C1/B1 would minimize initial code but force later safety/compatibility rewrites; A2+B2+C3 do not demand an exhaustive universal operation language, metadata schema generator, distributed CRDT, event-sourced plan log, shell process manager, or self-hosted GUI framework. Later S3 should reuse the same semantically versioned plan/running identity, with independently specified typed data edges and handler adapters, **not** silently reinterpret S2 prerequisites.

## 4. Research-only executable experiment

Location: `crates/riesel_app/tests/visual_authoring_authority_research.rs`. Run:

```sh
cargo test --locked -p riesel_app --test visual_authoring_authority_research
cargo validate --check
cargo smoke
```

**Verified result (2026-11-22):** [Linux Validate run 44993590466](https://github.com/aschenrot/riesel/actions/runs/44993590466) **PASSED** `cargo validate --check` and `cargo smoke` on exact GitHub synthetic merge `ee23f58` (research source head `b64434be9f4b2dcf00ce8e58c3920dc7400ab218`, then-main `a0500af86e0f53815ca0432f965a2232f9dcd6f2`). All **9/9 test cases** in `visual_authoring_authority_research.rs` passed. The additional runtime ingress research workflow [44993590464](https://github.com/aschenrot/riesel/actions/runs/44993590464) passed on the same source head. The first attempt [44993516660](https://github.com/aschenrot/riesel/actions/runs/44993516660) failed formatting only; the generated rustfmt diff was applied without changing intended assertions. Windows release validation and independent cargo execution outside GitHub Actions were **not** performed. These results verify in-process test semantics, not production remote authorization or crash-safe publication. This report may receive a docs-only follow-up commit, in which case that exact new head requires its own CI check before being considered current integration evidence.

| Test | Positive/negative property | What it does **not** prove |
| --- | --- | --- |
| `production_bind_alone_does_not_verify_required_resource_or_domain` | Real `bind` accepts a forged plan with a removed mandatory write access or changed admission owner. Test-only publisher policy gate rejects it. | No production descriptor enforcement or proof of actual Rust borrow safety. |
| `trusted_descriptor_rejects_forgery_and_checks_exact_version_without_effects` | Test-only artifact/policy rejects missing write, wrong domain, missing key/version and wrong purported artifact; callbacks untouched. | String artifact ID is not a verified executable signature or principal; no independent client security. |
| `unsupported_s3_required_features_are_not_dispatched_or_lossily_rewritten` | Strict read format/required feature failure, preserved original JSON, zero version key fails. | No chosen public schema, S3 typed runtime or compiler-native port check. |
| `concurrent_writers_only_one_semantic_update_wins` | Two synchronized threads race against one test-local authority; only one exact expected semantic predecessor wins; active revision remains old. | Single-process mutex is not durable/CAS under multiple OS processes. |
| `layouts_do_not_mutate_semantics_and_reject_missing_nodes_and_stale_edits` | Layout-only saves never change semantic revision/active plan; rejects invalid anchors, stale layout and removed node. | Layout rebase across real editor/format versions and atomic persistent storage. |
| `explicit_source_handoff_refuses_loss_and_fences_old_writers` | Test-only incomplete-coverage handoff refused; successful transfer increments source generation and rejects old writes. | A boolean is not a full Rust construct coverage proof or verified actor authorization. |
| `candidate_publication_is_distinct_from_activation_and_uncertain_reply_can_resnapshot` | Candidate inactive until separately permissioned activation, unauthorized attempt no effect, local receipt identifies already-applied request after hypothetical lost reply. | This is **in-memory** state only; it does not prove a crash-safe journal, replay-safe external effects or exactly-once semantics. |
| `accepted_semantic_revisions_stay_stable_when_later_candidates_are_published` | Prior real validated plan description and active revision unchanged after publishing a new candidate. | No actual live graph runner hot-update capability. |
| `separate_view_does_not_invent_value_edges` | Real production graph depends on control predecessor; layout/authoring cannot introduce typed transfer. | No S3 typed data execution or performance data. |

**Expected negative path finding, to validate in CI:** current real SDK binding permits an operation plan missing its handler's compulsory write access; that is why a **publisher-authoritative** editability/constraint gate must be specified *before* executable visual editing. No test should change that underlying SDK without an accepted production design.

### Persistent authority / conditional-write comparator — second research fixture

To avoid confusing the in-memory `LocalLedger` with an actual durable writer, a second **test-only** probe uses the repository's existing SQLite research dependency in `packages/storage/sqlite/tests/visual_plan_persistence_research.rs`. It stores the real validated plan's serialized **read-only projection** as a candidate payload, with separate semantic/head, layout and activation receipt records. The fixture uses SQLite immediate transactions, conditional SQL updates, a unique candidate key and independent reopened connections; it is deliberately **not** a public editor publisher or executable document importer.

Reproduce at the exact PR revision:

```sh
cargo test --locked -p riesel_storage_sqlite --test visual_plan_persistence_research
cargo validate --check
cargo smoke
```

**Verified result (2026-11-22):** [Linux Validate CI 44993997148](https://github.com/aschenrot/riesel/actions/runs/44993997148) **PASSED** `cargo validate --check` and `cargo smoke` on exact synthetic merge `3153b46` (source head `79ede184fb4986e510c2e66dc522a3a498a1069e`, then-current `main` `a0500af86e0f53815ca0432f965a2232f9dcd6f2`). All **5/5** persistent-store tests passed (in addition to the **9/9** original SDK editor-gate tests). The extra runtime-ingress research [CI 44993997169](https://github.com/aschenrot/riesel/actions/runs/44993997169) passed on that source as well. An earlier run [44993935075](https://github.com/aschenrot/riesel/actions/runs/44993935075) failed rustfmt only; its exact formatter diff was applied. No Windows release checks were run. This report may receive a docs-only follow-up revision, so verify the final head's own CI separately.

| Persistent test | Verified on real SQLite and production plan projection | Still not proven |
| --- | --- | --- |
| `independent_connections_race_on_the_same_persistent_semantic_revision` | Two independent connections race under transactions; **exactly one** conditional write commits; old active plan stays at revision 1 | Multiple **OS process** hostile writers; network/distributed CAS |
| `separate_layout_cas_and_source_generation_survive_connection_reopen` | Layout-only updates do not change semantic revision; a source-epoch change fences stale Rust and layout writers after reopening; historic semantic layout anchors can remain valid | Real verified owner identity; all-node layout rebase; durable source migration coverage report |
| `uncommitted_activation_rolls_back_on_connection_loss` | Dropping an uncommitted SQLite transaction leaves previously committed plan/active state unchanged on reopen | Actual OS-process kill, power loss or external business effect rollback |
| `published_candidate_and_activation_receipt_survive_reopen_after_lost_reply` | Candidate and active revision separate; unauthorized candidate activation denied in test-local policy; a committed activation receipt is queryable after connection reopen and rejects mismatched replay | Verified client/grant, runtime session fence, durable multi-process app publisher protocol or exactly-once effects |
| `persistent_candidate_uses_the_actual_read_only_validated_plan_projection` | The stored candidate is derived from the **real SDK plan description** | Stored JSON is still **not an executable import ABI**, signed provenance, compatible schema or executable binding authority |

**Storage alternatives / cost:** A single application-owned local transactional store can enforce *local* revision predicates and receipt co-commit (demonstrated), but would tie the authoring authority to that store's lifecycle and require explicit replication/backup/recovery for remote hosting. A filesystem atomic rename plus lock/generation protocol may be sufficient for a single trusted writer yet has nontrivial multi-writer and fsync failure cases (not tested). An append-only event/command log could improve audit, but adds replay, retention, compaction and order requirements and is not compelled by first-release visual S2 editing (not tested). A mandatory Manager-owned registry would duplicate app execution ownership and block independent headless workspaces. **This experiment selects no storage provider**; it proves a transaction-capable local adapter is plausible and narrows the questions about where verified app/artefact owner and session-grant authority reside.

**Critical semantic caveat:** Co-committing an `active_revision` pointer and a receipt in *one SQLite database* does **not** prove safe retry of an executable operation or remote side effect. A real application activation adapter must link a durable identity and live runtime generation, distinguish an activation request accepted **before** dispatch from a later uncertain outcome, and must not mistake a stored pointer or missing acknowledgement for confirmed running work. Actual restart/recovery and hostile-client tests remain mandatory.

## 5. Security, durability and scale unresolved

- **Authorization proof:** An artifact name/string or same-UID process cannot be a remote principal; profile-specific non-self-selectable principals/grants under #75/#60 are necessary for hostile-client environments. A2 inert metadata must be bound to the *actual* trusted installed artifact/registry, and differing artifact versions must fail closed.
- **Revision allocation and ownership:** V2 requires owner-fenced expected revisions for semantics and layout. A one-process mutex is insufficient for atomic compare-and-swap across multiple editor processes or after crashes. Select persistence, authoritative revision store and atomic commit protocol only after an adversarial test of out-of-process concurrent writers and recovery.
- **Source migration:** Every executable construct, exact handler version and owner/domain constraint must appear in a compatibility report. Arbitrary Rust source round-trip remains impossible; a failing report blocks handoff rather than dropping data.
- **Activation/restart:** Distinguish candidate persist, activation admitted, result confirmed and uncertain result. A simulated dropped reply with an in-memory receipt is *not* durable recovery; implement/verify real backend restart, receipt/journal persistence and session-generation fencing before public activation.
- **Resource/cost envelope:** Design explicit max document size, node/edge/dependency budget, metadata/catalog entries and validation CPU/memory budgets; keep per-run observer and session admission bounds. Measure representative small app, wide async DAG, large graph and delegated game/simulation tick; no numerical scaling claim is proved by this fixture.
- **S3 path:** Preserve S2-only plans with no typed buffers. Future typed ports require versioned handler adapters, type/lifetime/affinity proofs, explicit value-ready admission, serialized vs domain-local value alternatives and bounded backpressure. String matching and sample fields do not establish safe executable transfer.

## 6. Recommendation / owner acceptance gate

**Recommend A2+B2+C3 as one coherent architectural *direction***, with explicit boundaries, not pre-selected API types. The current unit experiment demonstrates feasible *in-memory semantics* and exposes unsafe direct registry inference, but it does not settle OS-trusted external publication, durable CAS/journal or runtime activation. Those are significant testable implementation/security gates.

Before declaring #111 complete, the owner should **accept / revise / defer** this consolidated direction and its actual scope, especially: passive independently versioned descriptor versus live-only metadata; strict required-feature policy; app-owned authority rather than mandatory Manager; permissioned proposal/publish/activate separation; durable stale-write and outcome-unknown guarantees. Then update the existing product/execution-plan architecture contract, and create separately verifiable implementation issues that preserve headless Rust-first operation, C, S2, V2 and future S3 compatibility.

**Do not** merge these research fixtures into production SDK as a contract, use the test ledgers as a real secure publisher, or claim a graphical editor is already delivered. [Future typed dataflow investigation #187](https://github.com/aschenrot/riesel/issues/187) is a separate deferred specification gate, not a prerequisite to implementing accepted S2 once #111 is fully decided.
