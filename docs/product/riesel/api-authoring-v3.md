# Riesel — Accepted Application Authoring Contract (v3)

**Status:** Stages 5.1–5.7 implemented and Linux-verified, 2026-11-19. This document defines durable API and composition rules, not a record of each implementation commit. See [executable high-level examples](high-level-app.md) for exact current syntax and supported features.

## Mental model

**Application uses capabilities; component implements capabilities; Riesel composes and runs them.** An application is a logical unit of behavior and lifecycle, not inherently one OS process. A component is an implementation, not intrinsically a process or a remotely deployed service.

The high-level `#[riesel::app]` macro is the convenience authoring surface for ordinary asynchronous applications. It must use the same schema, provider resolver, session ownership, error semantics and deployment preflight as the explicit `Application` builder. Use the builder for dynamic or specialized composition.

## Contract and identity

- `#[riesel::capability]` describes a typed, versioned contract. A private capability can derive its identity from its fully qualified Rust declaration path; moving or renaming it changes that private identity.
- Independently published contracts require an intentionally stable external ID and an independently managed version. Do not silently rename Counter or Storage contract IDs.
- Component definition, concrete provider identity, running instance identity, requirement alias and capability contract identity are distinct. Ports and Rust variable names do not substitute for these identities.
- A component declares its provided contracts once. `#[riesel::component]` generates linked/local and managed adapters from that declaration. One component instance can provide multiple interfaces backed by shared state.
- A component constructor can use a factory; `Default` is a convenience, not a mandatory architectural contract.

## Requirement and provider selection

- Typed application parameters describe dependencies exactly once; explicit aliases select additional instances of the same contract when needed.
- Parameter variable names are not stable dependency identities. Missing, duplicate, ambiguous and incompatible bindings fail validation before starting providers.
- A linked provider must be statically registered. Managed and external placements require explicit trusted configuration or a preapproved provider catalog.
- Unambiguous linked selection may be automatic; no global hidden constructor, magic linker discovery, or implicit MemoryStore fallback is permitted.
- Operator selection manifests can choose only preapproved placement names, not arbitrary executables or endpoints. Inspected plans are side-effect-free and preserve pinned provider metadata.
- Rust-generated application definitions are inspectable metadata; ordinary Rust closures or functions are not serialized as executable schemas.

## Execution and ownership

- Generated native async capability handles work with linked providers and managed nonblocking TCP without introducing a second runtime or transport-specific application API.
- Application-session shutdown invalidates cloned typed handles. A new recovery session creates new handles and task scopes; it does not silently reconnect old clones.
- Call failures must retain distinct domain, transport, cancellation and session-expiry information. `AppResult` may ease error propagation without discarding causes.
- A dropped RPC future or timeout does not guarantee that a transmitted command was not executed. Riesel must not automatically replay ambiguous commands.
- Native async work must be genuinely nonblocking. Provider-owned blocking operations use an explicit, bounded worker boundary and cooperative shutdown.
- Multiple capability interfaces on one component share state and lifecycle except where explicit, validated configuration says otherwise.

## Modules and application lifecycle

Optional `ApplicationModule` registrations reuse the same builder and dependency DAG. Module registration ordering is **not** a substitute for declared dependency ordering. Repeated or conflicting registration is rejected before startup.

Optional named start/stop hooks now use the existing application session, typed parameter extraction and capability dependency DAG. A general scheduler remains outside Stage 5.7. See [application modules](application-modules.md) for precise execution and recovery guarantees, and the [roadmap](implementation-roadmap.md) for the verification gate.

## Safety and extensibility boundaries

- Current plaintext RPC is restricted to loopback; contract metadata and a readiness announcement are **not** cryptographic peer authentication.
- Durable Storage, authenticated remote transport, authorization, observability and safe stateful recovery need separate production acceptance.
- Independent native window hosts are a product requirement, not an existing runtime feature.
- ECS, multiplayer, event subscriptions, messaging and UI frameworks are possible specialized extensions, not mandatory core dependencies.

## Acceptance evidence

Counter, Todo, catalog selection, macro/builder equivalence, module composition and session-hook integration tests were Linux-verified through Stage 5.7. Refer to the [active roadmap](implementation-roadmap.md) for stage gates and to [high-level app authoring](high-level-app.md) for actual runnable syntax.

Do not mark an API complete based on an illustrative example alone; executable tests, versioned contracts and CI are authoritative.
