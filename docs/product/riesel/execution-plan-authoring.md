# Headless execution-plan authoring (foundation)

**Status:** The versioned-plan and binding foundation is implemented in Riesel's Rust SDK. A bounded, one-shot **session-owned async graph runner** is available behind the `async-tcp` feature ([current runner scope](execution-runner.md)), but full runner acceptance and authoritative state/control are still open. A graphical inspector is not yet delivered. This authoring surface is not an executable manifest or stable JSON wire format.

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

## Boundaries and next steps

- This graph is **not** the capability/provider startup dependency graph, the `AppSchema`, or the provider `DeploymentPlan`. Existing provider selection and session identity remain authoritative.
- Resource declarations do not establish Rust borrowing safety. A signal declaration does not independently create an event source or execute a node. This **authoring-only foundation** provides no typed value transfer, clock policy, runtime scheduling, cancellation, retry, GUI or remote authorization by itself. The separate session-owned async runner supports bounded one-shot execution and cancellation within its documented limits.
- A new semantic revision must be published for a changed plan. Catalog immutability holds within that catalog's lifetime, not across trusted or untrusted processes.
- The implemented [session-owned runner (#112)](execution-runner.md) invokes supported bound async operations under the existing application lifecycle; remaining #112 domain-policy acceptance is tracked in its [implementation issue](https://github.com/aschenrot/riesel/issues/112). [Authoritative run inspection/control (#113)](https://github.com/aschenrot/riesel/issues/113) and [optional visual inspection (#114)](https://github.com/aschenrot/riesel/issues/114) follow, with a separate bounded [visual editing specification (#111)](https://github.com/aschenrot/riesel/issues/111).

For accepted invariants see the [execution-plan contract](execution-plan-contract.md) and [delivery roadmap](implementation-roadmap.md). This example is intentionally **not** evidence of a production-ready runtime scheduler.
