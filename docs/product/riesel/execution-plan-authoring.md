# Headless execution-plan authoring (foundation)

**Status:** The versioned-plan/binding foundation, bounded one-shot session-owned async runner, read-only authoritative observation, owner-confirmed effect prefixes and cooperative outer-admission controls are implemented in the reference SDK. Runtime graph execution requires the `async-tcp` feature ([actual limits](execution-runner.md)); a graphical inspector is not delivered. This authoring surface is not an executable manifest or stable JSON wire format.

## Run the example

From the repository root with Rust 1.100.0:

```sh
cargo run -p riesel --example execution_plan
```

See [the complete Rust example](../../../crates/riesel/examples/execution_plan.rs). It constructs a plan with a signal-gated source operation, a dependent operation, a symbolic resource-access declaration and exact-version Rust bindings. It prints the read-only plan description and stable topological order. **No registered operation executes** in the example; validation, binding and catalog publication are deliberately side-effect-free.

## Supported authoring steps

1. Construct `OperationKey::new(name, nonzero_version)` for each registered Rust behavior.
2. Register exact-version application-owned handlers in `OperationRegistry<H>`. The application chooses the type `H` (including a trait object for its handlers).
3. Use `ExecutionPlanDraft::new(logical_id, nonzero_semantic_revision)` with `OperationNode::new(id, key)`; optionally declare `.after(prerequisite)`, `.on_signal(signal)`, and `.access(resource, AccessMode::Read | AccessMode::Write)`.
4. Call `.validate()` to obtain `ValidatedExecutionPlan`. It rejects duplicate IDs, missing/self/repeated dependencies, cycles, malformed signal/resource declarations and invalid revisions without invoking code.
5. Call `.bind(&registry)` to require exact matching registered handlers for every node. This is lookup-only and returns `BoundExecutionPlan<H>`; it does **not** start work. Optionally publish the immutable validated plan in a local `PlanRevisionCatalog` to reject reuse of the same logical ID/revision.
6. Inspect `description()` or `dependency_order()`. The `serde::Serialize` projection is read-only metadata. **Do not deserialize it as an executable plan** or treat it as proof of app identity or authorization.

## Declared domain boundaries

A plan can optionally label outer simulation operations with
`OperationNode::domain_tick("simulation")` (Riesel admits the outer tick),
or `OperationNode::delegated_frame("simulation")` (the domain owns its
internal tick/system schedule). Each label appears in read-only plan
inspection. Validation rejects invalid domain names and conflicting
owners of the same domain *within* one immutable plan. The runtime's
session admission additionally rejects conflicting authorities across
outstanding runs and serializes declared domain operations even without
a resource key. This declaration does **not** create an ECS integration,
clock, separate supervisor or automatic safe-point debugger; see the
[execution runner contract](execution-runner.md).

## Reusing immutable plan bindings for distinct tick runs

A validated, bound plan can now be cloned without executing any operation
or re-resolving capability providers. Each `RunningApp::execution_graph`
call accepts the same immutable plan revision as a **separate one-shot
execution**. The [fixed-step admission policy](execution-runner.md) can
supply an explicitly bounded number of tick proposals per externally
sampled frame; it is not an automatic graph loop or proof of committed
side effects. Session-wide domain authority and resource admission
continue to apply separately to each runner. Runtime-issued run identity and attempt correlation are implemented by
the read-only #113 observer. Explicit owner-confirmed cumulative effect
prefixes are available as a bounded #113 increment; a claimed tick
proposal or successful callback is not proof of an external mutation.

## Trusted-local editor-safe operation registration (first A2 increment)

The accepted [#111 architecture](execution-plan-contract.md) requires editor-selectable operations to have **application-owned** descriptors, distinct from arbitrary code-first `OperationRegistry::register` calls. The initial trusted-local SDK path is `OperationRegistry::register_editor_operation` and `OperationRegistry::bind_editor_plan`:

```rust
use riesel::{
    AccessMode, EditorOperationDescriptor, ExecutionPlanDraft, OperationKey,
    OperationNode, OperationRegistry,
};
use std::sync::Arc;

let key = OperationKey::new("example.update", 1)?;
let authoring = EditorOperationDescriptor::new(key.clone())
    .require_access("state", AccessMode::Write)
    .require_signal("ready");

let mut handlers = OperationRegistry::<dyn Fn() + Send + Sync>::new();
handlers.register_editor_operation(authoring, Arc::new(|| {}))?;

let candidate = ExecutionPlanDraft::new("example.plan", 1)
    .operation(OperationNode::new("update", key)
        .access("state", AccessMode::Write)
        .on_signal("ready"))
    .validate()?;

// This step checks compulsory trusted resources and permitted triggers
// *before* binding the exact Rust callback, without executing the callback.
let bound = handlers.bind_editor_plan(&candidate)?;
assert!(bound.handler("update").is_some());
```

This is a **trusted application process registration and preflight**, not a remote editor exporter, persisted document format, credential, code generator or publication/activation permission. The descriptor is inert/Serialize-only. It cannot prove the true Rust aliasing/affinity behavior of an opaque closure; the **application/domain author must attest** compulsory resource and domain constraints. Unsupported native-thread-affine editing fails closed until a verified owner adapter exists. Existing `register` and `ValidatedExecutionPlan::bind` remain valid for trusted Rust code-owned plans but **must not be used alone to approve untrusted document-owned edits**.

The first A2 increment supports mandatory resource access, separately permitted optional access, permitted immediate/signal triggers and one fixed required domain boundary. It does not allow arbitrary reclassification of a domain owner or native `!Send` state. Strict B2 executable document parsing, V2 persistent ownership/CAS and C3 app-owned publication/activation are [separate tracked implementation increments](https://github.com/aschenrot/riesel/issues/111). S2→S3 typed value edges remain a separate research gate in [#187](https://github.com/aschenrot/riesel/issues/187).

## Boundaries and next steps

- This graph is **not** the capability/provider startup dependency graph, the `AppSchema`, or the provider `DeploymentPlan`. Existing provider selection and session identity remain authoritative.
- The authoring-only `execution_plan` example never invokes callbacks. The separate `execution_graph`, `execution_fixed_step` and `execution_control` examples demonstrate real bounded execution, deterministic external-clock tick proposals, and cooperative **outer** safe-point stepping respectively; all are still headless.
- Resource declarations do not establish Rust borrowing safety. A signal declaration does not independently create an event source or execute a node. This **authoring-only foundation** provides no typed value transfer, clock policy, runtime scheduling, cancellation, retry, GUI or remote authorization by itself. The separate session-owned async runner supports bounded one-shot execution and cancellation within its documented limits.
- A new semantic revision must be published for a changed plan. Catalog immutability holds within that catalog's lifetime, not across trusted or untrusted processes.
- The implemented [session-owned runner (#112)](execution-runner.md) invokes supported bound async operations under the existing application lifecycle. [Authoritative run inspection, effects and local cooperative controls (#113)](https://github.com/aschenrot/riesel/issues/113) reuse that same runner and do not grant external client authority. [Optional visual inspection (#114)](https://github.com/aschenrot/riesel/issues/114) is a separate supported-product goal; the [accepted visual editing specification (#111)](https://github.com/aschenrot/riesel/issues/111) now gates separately tracked executable editor implementation, beginning with trusted-local catalog safety.

For accepted invariants see the [execution-plan contract](execution-plan-contract.md) and [delivery roadmap](implementation-roadmap.md). This example is intentionally **not** evidence of a production-ready runtime scheduler.
