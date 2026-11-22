# GUI publication decision evidence — three application shapes (#248)

**Status:** Research comparison, not an accepted specification or production API.
**Baseline:** `main` `e1a4bcacc08ea9c6833a49b090fceaf9e553af4a` (22 Nov 2026).
**Earlier two-app probe:** [draft PR #91](https://github.com/aschenrot/riesel/pull/91)
at `cf569ea8d2747ab65a8d28006c1048f9336396bc`. This branch carries its
source policy/test comparator forward to the current runtime and adds a third,
actual graph-owner case. The unchanged earlier proof remains independently
reviewable; this research must not silently become the public SDK.

## Original outcome and growth case

Headless Rust-first applications publish *selected* versioned domain operations
and read models to independently operated browser/native/Bevy/custom GUIs.
No mandatory GUI, Manager, render toolkit, app process manager or new provider
resolver. Credible growth: independent Cargo apps, multiple GUI clients and
view adapters per app, cross-process attach/restart, mixed read/write grants,
large typed domain assets and S2/S3 plan evolution. A GUI closing does not
terminate backend logic. A UI is not itself automatically a capability/provider.

## Three verified-by-test designs under one policy shape

The test scenario uses a single research-only `ResearchExport` for contract
selection, simulated generation, principal/grant policy, pre-dispatch
denial, revocation and uncertain mutation classification. These are
*in-process* assertions; they do not verify peer authentication or delivery.

| SDK-owned case | Actual source of state | App-visible shape | Forwarding work | What test covers |
| --- | --- | --- | --- | --- |
| Counter | `AsyncCapability<AsyncCounterContract>` from real SDK resolution | Existing 3-op versioned Counter contract | Explicit `AsyncRpcHandler` three-method relay because resolved handles expose `CallError` rather than implement the provider trait | Real `value/add`, forbidden `label`, contract/principal denial, withdrawal |
| Todo | Real `Records<TaskData>` from application handler | Intentional `TodoPublic` v1 create/count | Existing generated dispatcher wrapping a domain facade | Domain success/failure, policy generation rejection, explicit revocation |
| Graph | `context.execution_graph` within real app session, read through `GraphRunObserver` | Intentional `GraphRead` v1 typed summary query | Existing generated dispatcher wrapping read-only observer | Before/after actual dependency execution, authoritative identity and phase, unknown operation, revocation |

The graph's one-query **summary is not the full editor/inspector projection**:
it does not carry detailed node descriptors, nested domain typed views or a
complete event log. Those are separately owned by the headless graph API and
[offline/live Chromium research PR #250](https://github.com/aschenrot/riesel/pull/250).
The two research adapters do *not* yet attach via one cross-process policy.
A shared gateway can be applied without a second resolver, but this is **not**
proof it eliminates all app-specific mapping or that all operations should
share one universal serialization format.

## Alternatives against the same target scope

| Choice | Full-scope fit | Benefits | Remaining cost/falsification |
| --- | --- | --- | --- |
| A. Per-app HTTP/native adapter | Possible, if each recreates correct policy | Direct app/UX ownership, flexible transport | Repeated grants, session checking, discovery, error outcomes and lifecycle; high multi-app migration risk |
| B. Generated app-public facade using existing capability host | Potentially, if app explicitly binds it to *its own* resolved state | Typed contracts, no second business schema | Generic provider RPC has no verified app-issued generation or peer identity; direct exposure bypasses app authorization; forwarding still exists for SDK-resolved handles |
| C. Opt-in application-owned publication/attachment boundary over selected versioned facades | Broadest plausible shared operational fit | Central reusable lifecycle/grant/fence and read/protocol semantics; host-neutral and app-authoritative | New public API, security validation, adapter cost, contract compatibility, risk of universal service overreach |
| D. Universal rendering or GUI-as-provider | Only when an independently justified renderer service is an app capability | May fit selected remote presentation systems | Generic widget protocol forces accessibility/input/render/serialization ownership on Riesel; does not itself solve client grants or backend identity |
| E. Mixed shared application semantics + host-specific adapters | Strong hypothesis combining B/C and independent frontends | Limits platform scope while permitting native/web/specialized hosts | Must demonstrate reuse across the same actual client security/restart contract, not just a test policy |

No candidate earns acceptance solely from fewer lines or more layers.
[Source count script](measure.py) measures named fixtures; Counter/Todo HTTP
and graph summary do not provide equal operation sets, so no normalized LOC
reduction, transport throughput or general GPU performance claim is valid.

## Security and lifecycle falsification from existing research

- [#75 accepted operational semantics](../../docs/product/riesel/operational-contract.md)
  distinguish runtime-issued generation, app/artifact identity, client identity,
  per-operation grant and definitive vs unknown outcomes. This does **not**
  select a protocol.
- [#157 integrated internal ingress](https://github.com/aschenrot/riesel/issues/157)
  supplies runtime-owned session fences and owned admission, not client
  authentication or a public export API. Treat its hidden internal proof as a
  possible implementation reuse, not an automatically authorized client.
- [#158 integrated evidence](https://github.com/aschenrot/riesel/issues/158)
  distinguishes listener bound, app entrypoint running and actual
  authorized-operation callable readiness; the last is currently *unsupported*.
- [#87](https://github.com/aschenrot/riesel/pull/87) shows stale TCP binding
  and reply-loss counterexamples. No non-idempotent write retries without
  authoritative reconciliation.
- [#149](https://github.com/aschenrot/riesel/pull/149) proves a real
  private-network Counter + AF_UNIX UID grant and proves a **same-UID second
  process** can invoke that grant. [#151](https://github.com/aschenrot/riesel/pull/151)
  proves a client can self-enter a permissive trusted AppArmor profile. Thus
  UID, loopback, Origin/Host and self-selectable labels cannot attest a unique
  protected GUI client. `SimulatedPrincipal` in these tests is not a
  security mechanism.
- [#90](https://github.com/aschenrot/riesel/pull/90) demonstrates separate
  native GUI lifecycle under a real event loop; [#250](https://github.com/aschenrot/riesel/pull/250)
  demonstrates live browser graph rendering. Neither proves a production
  authorized attachment, safe session rebind or a portable protected principal.

**Additional local policy race, from source review:** `ResearchExport::invoke`
reads the `accepting` atomic and later awaits `handler.dispatch_for` without
a shared admission permit/lock. Concurrent `revoke()` can occur between
that check and actual dispatch. The sequential revocation tests do **not**
prove linearizable revocation-before-dispatch, nor do they cover draining
in-flight work. A production adapter must compose the actual #157
runtime-owned ingress permit with a revocable publication/grant admission
policy, and deterministically test both race orderings. Do not promote the
simulated gateway to production based on passing the three-app tests.

**Trust separation needed for acceptance:** explicitly state a constrained
trusted-local/shared-OS-user profile when its real limits are accepted for
non-sensitive reads. Sensitive reads and cross-client mutations require a
different, independently verified principal/grant profile. Do not mislabel
either as strong same-UID isolation, nor grant GUI clients Manager privileges.

## Conditional architecture direction, not owner acceptance

A reusable *application-owned* opt-in publication policy applied to selected
versioned typed facades is provisionally justified for the full product
reuse goal, while rendering/layout/input remains with each GUI host.
Do not introduce a second provider resolver, process owner, universal
widget tree, mandatory broker or reflection-based arbitrary capability dump.
Preserve app-domain ownership of snapshots and commands; let adapter
transports provide independently tested client evidence and bound payloads.
Allow a separate explicit authorized artifact/view service for large Grid,
Bevy/GPU and future typed S3 workflows without pretending native resources
are generic JSON.

**Adoption/migration path:**
1. Retain Todo's working HTTP API and native Counter research, with no
   mandatory migration. Offer optional app-selected public interfaces alongside.
2. Define server-owned publication generation, exact interface/operation
   catalog, grants, bounded snapshots and honest unknown outcomes. Use the
   existing typed code generator where it eliminates real forwarding work.
3. Verify an actual independent browser and native client under the **same
   chosen identity and session rules**, including same-PID recovery, drop/loss,
   denied operations, bounded subscriptions or explicit resnapshot, and multiple
   clients. Only then claim the accepted host boundary and finish #114.
4. Integrate S2 source/layout revisions, app-authoritative publication and
   separately authorized activation (#195/#196); keep future S3 views
   feature/version qualified and incompatible clients read-only.

## Material outstanding tests before accepting #248

- Real externally verifiable application runtime epoch **and client principal**
  at the publisher's ingress, not self-reported descriptor fields.
- Actual revoked/wrong/stale/other-client admission before dispatch across
  app restart **in the same PID**, and recovery of an independent GUI.
- Same selected operation surface in a real Counter, Todo and graph
  cross-process client; demonstrate no inadvertent internal provider exposure.
- Lost mutation acknowledgement → outcome unknown and authoritative
  reconciliation, never implicit replay.
- Bounded source/trace loss with full resnapshot and distinguished run/semantic
  and layout revisions; tested reconnect and multi-client load.
- OS trust portability evidence for the actual claimed protection level;
  keyboard/screen-reader and native host behavior where supported.

**Maintainer choices:** accept/revise/defer/reject the target ownership
boundary and explicit first trust profile. Neither this test nor passing CI
substitutes for the user's architecture decision. Keep [#248](https://github.com/aschenrot/riesel/issues/248)
open and [#114](https://github.com/aschenrot/riesel/issues/114) incomplete.
