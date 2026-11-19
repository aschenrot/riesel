# Riesel — Provider Catalog and Selection

Status: Stage 5.5 complete on Linux (2026-11-19).
Linux CI: [44503685582](https://github.com/aschenrot/riesel/actions/runs/44503685582).

## Purpose

A provider definition identifies an implementation and the exact versioned
capability contracts it declares. A **placement** identifies an approved way
to reach that implementation. One provider may be linked into an application,
run as a manager-owned child or be operated as an external service.

`ProviderCatalog` is a catalog of **allowed remote/managed placements**,
not a second container, registry or resolver. The regular
`ConnectorRegistry`, `DeploymentConfig` and preflight planner remain
authoritative.

## Developer registration

```rust
use riesel::{Application, ProviderCatalog, ServiceEndpoint};

// The provider definition comes from the same Rust component declaration
// that hosts the managed service.
let published = memory_async_counter_provider()?.definition()?;
let mut catalog = ProviderCatalog::new();
catalog.register(
    "managed",
    published,
    ServiceEndpoint::Managed {
        instance: None,
        program: "counter-service".into(),
        args: Vec::new(),
        startup_timeout_ms: 10_000,
        shutdown_timeout_ms: 2_000,
    },
)?;
```

The program path above is illustrative. A production deployment must choose
an executable path controlled by the application operator. Named managed
placements assign a stable instance key; several capability requirements
selecting the same placement share one managed child.

An unambiguous statically linked component is the only implicit default.
Remote and managed placements must be **selected explicitly**, never guessed
from a network, filesystem or build environment.

```rust
let prepared = Application::new("Counter")
    .async_handler_with_aliases(handler, ["counter", "observer"])?
    .component::<AsyncMemoryCounter>()?
    .catalog(catalog)
    .select_provider("counter", "managed")?
    .select_provider("observer", "managed")?;

// No component factory, process or network endpoint starts during inspection.
let plan = prepared.catalog_plan()?;
println!("{}", serde_json::to_string_pretty(&plan)?);
prepared.run_from_env()?;
```

## Operator selection-only manifest

`ProviderSelectionConfig` accepts only chosen placement names and
whole-application recovery policy. It does **not** accept executable,
address or port declarations:

```json
{
  "services": {
    "counter": "managed",
    "observer": "managed"
  },
  "recovery": {
    "mode": "fail_fast"
  }
}
```

Use `--select-providers selections.json` at application startup. Use
`--inspect-catalog --select-providers selections.json` (either argument
order) to inspect exact requirement aliases, startup order, provider pins and
placements without starting providers. `--deployment` and provider-catalog
selection are mutually exclusive configuration sources.

`ProviderSelectionConfig::load(path)` and
`Application::provider_selections(config)` offer the same behavior in code.
Unknown fields, undefined aliases, incompatible capability versions,
unknown placements, ambiguous linked providers and conflicting configuration
sources are rejected before side effects. Session-restart recovery is opt-in
and never retries individual commands after ambiguous transport failures.

## Security and lifecycle boundary

An external plain-TCP provider may be selected only over loopback until an
authenticated remote transport is implemented. Provider identity pins validate
**advertised contract metadata**, not authenticated peer identity. A managed
child belongs to Riesel and is supervised and stopped with its session; an
operator-owned external provider is not stopped by Riesel.

Avoid treating the Stage 5.5 catalog as a general-purpose service discovery
protocol. Secure transports, authorization and production isolation belong
to Stage 6.
