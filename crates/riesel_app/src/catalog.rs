//! Explicit, side-effect-free provider placement selection.
//!
//! Linked components remain registered in ConnectorRegistry. A catalog adds
//! operator-approved managed or loopback TCP placements of published provider
//! definitions. Its names identify *placements*, not provider identities.
use crate::{ConnectorRegistry, DeploymentConfig, DeploymentError, ServiceEndpoint};
use riesel_schema::{AppSchema, ProviderDefinition, ProviderError, SchemaError};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("invalid catalog choice {0}; use a lowercase identifier")]
    InvalidChoice(String),
    #[error("catalog choice already registered: {0}")]
    DuplicateChoice(String),
    #[error("catalog choice {0} collides with a linked provider identity")]
    CollidingChoice(String),
    #[error("invalid catalog provider definition: {0}")]
    InvalidProvider(#[from] ProviderError),
    #[error("catalog choice {0} must be a managed or external TCP placement, not a local provider")]
    UnsupportedPlacement(String),
    #[error("catalog choice {0} points to unauthenticated non-loopback TCP")]
    InsecureRemote(String),
    #[error("catalog selects undeclared requirement {0}")]
    UnknownRequirement(String),
    #[error("requirement {requirement} selects an unknown placement/provider {choice}")]
    UnknownChoice { requirement: String, choice: String },
    #[error("choice {choice} does not implement capability {id}@{version} for {requirement}")]
    IncompatibleChoice {
        requirement: String,
        choice: String,
        id: String,
        version: u32,
    },
    #[error("no linked or catalog provider implements {id}@{version} for {requirement}")]
    MissingProvider {
        requirement: String,
        id: String,
        version: u32,
    },
    #[error("requirement {requirement} needs an explicit remote/managed choice: {choices:?}")]
    SelectionRequired {
        requirement: String,
        choices: Vec<String>,
    },
    #[error("requirement {requirement} has ambiguous linked providers: {choices:?}")]
    AmbiguousLocal {
        requirement: String,
        choices: Vec<String>,
    },
    #[error(transparent)]
    Schema(#[from] SchemaError),
    #[error(transparent)]
    Deployment(#[from] DeploymentError),
}

struct Placement {
    provider: ProviderDefinition,
    endpoint: ServiceEndpoint,
}

/// The declared remote and managed placements an operator permits an app to
/// use. Linked choices are discovered only from explicitly registered local
/// component factories. Nothing here starts or probes a service.
#[derive(Default)]
pub struct ProviderCatalog {
    placements: BTreeMap<String, Placement>,
}

impl ProviderCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a named, trusted-by-configuration placement. This is NOT
    /// cryptographic remote authentication. External unauthenticated TCP
    /// is allowed only on loopback until a secure transport exists.
    pub fn register(
        &mut self,
        choice: impl Into<String>,
        provider: ProviderDefinition,
        endpoint: ServiceEndpoint,
    ) -> Result<(), CatalogError> {
        let choice = choice.into();
        if choice.is_empty()
            || !choice.starts_with(|c: char| c.is_ascii_lowercase())
            || !choice
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            return Err(CatalogError::InvalidChoice(choice));
        }
        provider.validate()?;
        match &endpoint {
            ServiceEndpoint::Local { .. } => {
                return Err(CatalogError::UnsupportedPlacement(choice));
            }
            ServiceEndpoint::Tcp { address } if !address.ip().is_loopback() => {
                return Err(CatalogError::InsecureRemote(choice));
            }
            _ => {}
        }
        if self.placements.contains_key(&choice) {
            return Err(CatalogError::DuplicateChoice(choice));
        }
        self.placements
            .insert(choice, Placement { provider, endpoint });
        Ok(())
    }

    /// Turn explicit placement selections into the existing deployment
    /// configuration and metadata pins. The ordinary runtime preflight and
    /// connector resolver remain authoritative; this step has no side effects.
    ///
    /// Without a selection, only an unambiguous linked provider can be
    /// chosen. A remote or managed placement is never silently launched.
    pub fn deployment(
        &self,
        app: &AppSchema,
        connectors: &ConnectorRegistry,
        selected: &BTreeMap<String, String>,
    ) -> Result<DeploymentConfig, CatalogError> {
        app.validate()?;
        let locals: BTreeMap<String, &ProviderDefinition> = connectors
            .local_definitions()
            .map(|provider| (provider.id.clone(), provider))
            .collect();

        // Names are placement choices, not provider IDs. A collision with a
        // local provider ID would make one user selection ambiguous.
        for choice in self.placements.keys() {
            if locals.contains_key(choice) {
                return Err(CatalogError::CollidingChoice(choice.clone()));
            }
        }
        for requirement in selected.keys() {
            if !app.requires.iter().any(|item| item.name == *requirement) {
                return Err(CatalogError::UnknownRequirement(requirement.clone()));
            }
        }

        let mut services = BTreeMap::new();
        let mut expected = BTreeMap::new();
        for requirement in &app.requires {
            let alias = &requirement.name;
            let capability = &requirement.capability;
            let endpoint = if let Some(choice) = selected.get(alias) {
                if let Some(placement) = self.placements.get(choice) {
                    if !placement.provider.capabilities.contains(capability) {
                        return Err(CatalogError::IncompatibleChoice {
                            requirement: alias.clone(),
                            choice: choice.clone(),
                            id: capability.id.clone(),
                            version: capability.version,
                        });
                    }
                    expected.insert(alias.clone(), placement.provider.clone());
                    let mut endpoint = placement.endpoint.clone();
                    // One catalog placement represents one managed instance,
                    // even when multiple capability aliases select it.
                    if let ServiceEndpoint::Managed { instance, .. } = &mut endpoint {
                        if instance.is_none() {
                            *instance = Some(choice.clone());
                        }
                    }
                    endpoint
                } else if let Some(provider) = locals.get(choice) {
                    if !provider.capabilities.contains(capability) {
                        return Err(CatalogError::IncompatibleChoice {
                            requirement: alias.clone(),
                            choice: choice.clone(),
                            id: capability.id.clone(),
                            version: capability.version,
                        });
                    }
                    ServiceEndpoint::Local {
                        provider: choice.clone(),
                    }
                } else {
                    return Err(CatalogError::UnknownChoice {
                        requirement: alias.clone(),
                        choice: choice.clone(),
                    });
                }
            } else {
                let compatible: Vec<String> = locals
                    .values()
                    .filter(|provider| provider.capabilities.contains(capability))
                    .map(|provider| provider.id.clone())
                    .collect();
                match compatible.as_slice() {
                    [only] => ServiceEndpoint::Local {
                        provider: only.clone(),
                    },
                    [] => {
                        let choices: Vec<String> = self
                            .placements
                            .iter()
                            .filter(|(_, placement)| {
                                placement.provider.capabilities.contains(capability)
                            })
                            .map(|(name, _)| name.clone())
                            .collect();
                        if choices.is_empty() {
                            return Err(CatalogError::MissingProvider {
                                requirement: alias.clone(),
                                id: capability.id.clone(),
                                version: capability.version,
                            });
                        }
                        return Err(CatalogError::SelectionRequired {
                            requirement: alias.clone(),
                            choices,
                        });
                    }
                    _ => {
                        return Err(CatalogError::AmbiguousLocal {
                            requirement: alias.clone(),
                            choices: compatible,
                        });
                    }
                }
            };
            services.insert(alias.clone(), endpoint);
        }
        let mut deployment = DeploymentConfig::new(services);
        deployment.expected_providers = expected;
        deployment.validate(app)?;
        Ok(deployment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::{CapabilityRef, parse_schema};

    fn app() -> AppSchema {
        parse_schema(
            r#"{"version":1,"app":{"name":"Counter"},"models":{},
            "requires":[{"name":"counter","capability":{"id":"example.counter","version":1}}]}"#,
        )
        .unwrap()
    }

    fn provider(id: &str, version: u32) -> ProviderDefinition {
        ProviderDefinition {
            id: id.into(),
            capabilities: vec![CapabilityRef {
                id: "example.counter".into(),
                version,
            }],
        }
    }

    fn target() -> ServiceEndpoint {
        ServiceEndpoint::Managed {
            instance: None,
            program: "counter_service".into(),
            args: Vec::new(),
            startup_timeout_ms: 10_000,
            shutdown_timeout_ms: 2_000,
        }
    }

    #[test]
    fn remote_and_managed_are_never_implicit_and_selection_is_pinned() {
        let mut catalog = ProviderCatalog::new();
        catalog
            .register("managed", provider("example.counter.memory", 1), target())
            .unwrap();
        let choices = BTreeMap::new();
        let empty = ConnectorRegistry::new();
        assert!(matches!(
            catalog.deployment(&app(), &empty, &choices),
            Err(CatalogError::SelectionRequired { requirement, choices })
                if requirement == "counter" && choices == ["managed"]
        ));
        let choices = BTreeMap::from([("counter".into(), "managed".into())]);
        let config = catalog.deployment(&app(), &empty, &choices).unwrap();
        assert_eq!(
            config.expected_providers["counter"].id,
            "example.counter.memory"
        );
        assert!(matches!(
            &config.services["counter"],
            ServiceEndpoint::Managed { instance: Some(name), .. } if name == "managed"
        ));
    }

    #[test]
    fn catalog_rejects_invalid_choices_and_public_plaintext_endpoints() {
        let mut catalog = ProviderCatalog::new();
        assert!(matches!(
            catalog.register("Bad", provider("example.counter.memory", 1), target()),
            Err(CatalogError::InvalidChoice(_))
        ));
        assert!(matches!(
            catalog.register(
                "remote",
                provider("example.counter.memory", 1),
                ServiceEndpoint::Tcp {
                    address: "192.0.2.1:4000".parse().unwrap()
                }
            ),
            Err(CatalogError::InsecureRemote(_))
        ));
        catalog
            .register("remote", provider("example.counter.memory", 1), target())
            .unwrap();
        assert!(matches!(
            catalog.register("remote", provider("example.counter.memory", 1), target()),
            Err(CatalogError::DuplicateChoice(_))
        ));
        assert!(matches!(
            catalog.register(
                "linked",
                provider("example.counter.memory", 1),
                ServiceEndpoint::Local {
                    provider: "example.counter.memory".into()
                }
            ),
            Err(CatalogError::UnsupportedPlacement(_))
        ));
    }

    #[test]
    fn unknown_and_incompatible_placements_fail_before_startup() {
        let mut catalog = ProviderCatalog::new();
        catalog
            .register("managed", provider("example.counter.old", 2), target())
            .unwrap();
        let empty = ConnectorRegistry::new();
        assert!(matches!(
            catalog.deployment(
                &app(),
                &empty,
                &BTreeMap::from([("counter".into(), "managed".into())])
            ),
            Err(CatalogError::IncompatibleChoice { version: 1, .. })
        ));
        assert!(matches!(
            catalog.deployment(
                &app(),
                &empty,
                &BTreeMap::from([("counter".into(), "unregistered".into())])
            ),
            Err(CatalogError::UnknownChoice { .. })
        ));
        assert!(matches!(
            catalog.deployment(
                &app(),
                &empty,
                &BTreeMap::from([("missing".into(), "managed".into())])
            ),
            Err(CatalogError::UnknownRequirement(_))
        ));
    }
}
