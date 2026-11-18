use riesel_schema::{AppSchema, CapabilityRef, ProviderDefinition, ProviderError, SchemaError};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use thiserror::Error;

/// The actual instance owning one capability is independent of the provider
/// implementation ID. Distinct managed processes can use the same provider
/// definition without becoming the same live component instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstanceIdentity {
    Local { provider: String },
    Managed { name: String },
    ExternalTcp { address: SocketAddr },
}

/// A declarative requirement-provider match, optionally enriched by the
/// runtime with the concrete instance that owns the binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCapability {
    pub requirement: String,
    pub capability: CapabilityRef,
    pub provider: String,
    pub instance: Option<InstanceIdentity>,
}

#[derive(Debug, Error)]
pub enum ResolutionError {
    #[error("invalid application schema: {0}")]
    InvalidApplication(#[from] SchemaError),
    #[error("invalid provider definition: {0}")]
    InvalidProvider(#[from] ProviderError),
    #[error("provider already registered: {0}")]
    DuplicateProvider(String),
    #[error("binding refers to undeclared requirement: {0}")]
    UnknownBinding(String),
    #[error("no provider implements capability {id}@{version} required as {requirement}")]
    NoCompatibleProvider {
        requirement: String,
        id: String,
        version: u32,
    },
    #[error("requirement {requirement} selects unknown provider {provider}")]
    UnknownProvider {
        requirement: String,
        provider: String,
    },
    #[error(
        "provider {provider} does not implement capability {id}@{version} required as {requirement}"
    )]
    IncompatibleProvider {
        requirement: String,
        provider: String,
        id: String,
        version: u32,
    },
    #[error("requirement {requirement} has multiple compatible providers: {providers:?}")]
    AmbiguousProvider {
        requirement: String,
        providers: Vec<String>,
    },
}

/// An explicit, in-process catalog of provider declarations.
/// No dynamic discovery or service startup is implied.
#[derive(Debug, Default)]
pub struct ProviderRegistry {
    providers: BTreeMap<String, ProviderDefinition>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, provider: ProviderDefinition) -> Result<(), ResolutionError> {
        provider.validate()?;
        if self.providers.contains_key(&provider.id) {
            return Err(ResolutionError::DuplicateProvider(provider.id));
        }
        self.providers.insert(provider.id.clone(), provider);
        Ok(())
    }

    /// Match every declared requirement against exact capability IDs and versions.
    /// Bindings map requirement names to provider IDs; when omitted, exactly one
    /// matching provider must exist. Ambiguous matches are rejected.
    pub fn resolve(
        &self,
        app: &AppSchema,
        bindings: &BTreeMap<String, String>,
    ) -> Result<Vec<ResolvedCapability>, ResolutionError> {
        app.validate()?;
        for requirement in bindings.keys() {
            if !app.requires.iter().any(|item| &item.name == requirement) {
                return Err(ResolutionError::UnknownBinding(requirement.clone()));
            }
        }

        let mut resolved = Vec::with_capacity(app.requires.len());
        for requirement in &app.requires {
            let requested = &requirement.capability;
            let provider = if let Some(selection) = bindings.get(&requirement.name) {
                let selected = self.providers.get(selection).ok_or_else(|| {
                    ResolutionError::UnknownProvider {
                        requirement: requirement.name.clone(),
                        provider: selection.clone(),
                    }
                })?;
                if !selected.capabilities.contains(requested) {
                    return Err(ResolutionError::IncompatibleProvider {
                        requirement: requirement.name.clone(),
                        provider: selection.clone(),
                        id: requested.id.clone(),
                        version: requested.version,
                    });
                }
                selected.id.clone()
            } else {
                let compatible: Vec<_> = self
                    .providers
                    .values()
                    .filter(|provider| provider.capabilities.contains(requested))
                    .map(|provider| provider.id.clone())
                    .collect();
                match compatible.as_slice() {
                    [] => {
                        return Err(ResolutionError::NoCompatibleProvider {
                            requirement: requirement.name.clone(),
                            id: requested.id.clone(),
                            version: requested.version,
                        });
                    }
                    [only] => only.clone(),
                    _ => {
                        return Err(ResolutionError::AmbiguousProvider {
                            requirement: requirement.name.clone(),
                            providers: compatible,
                        });
                    }
                }
            };
            resolved.push(ResolvedCapability {
                requirement: requirement.name.clone(),
                capability: requested.clone(),
                provider,
                instance: None,
            });
        }
        Ok(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::parse_schema;

    fn app() -> AppSchema {
        parse_schema(
            r#"{
                "version": 1,
                "app": {"name": "Todo"},
                "models": {},
                "requires": [
                    {"name": "storage", "capability": {
                        "id": "riesel.storage.records", "version": 1
                    }}
                ]
            }"#,
        )
        .unwrap()
    }

    fn provider(id: &str, version: u32) -> ProviderDefinition {
        ProviderDefinition {
            id: id.into(),
            capabilities: vec![CapabilityRef {
                id: "riesel.storage.records".into(),
                version,
            }],
        }
    }

    #[test]
    fn resolves_exactly_one_compatible_provider() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(provider("riesel.storage.memory", 1))
            .unwrap();

        let result = registry.resolve(&app(), &BTreeMap::new()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].requirement, "storage");
        assert_eq!(result[0].provider, "riesel.storage.memory");
    }

    #[test]
    fn rejects_missing_or_incompatible_versions() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(provider("riesel.storage.memory", 2))
            .unwrap();
        assert!(matches!(
            registry.resolve(&app(), &BTreeMap::new()),
            Err(ResolutionError::NoCompatibleProvider { version: 1, .. })
        ));

        let selection = BTreeMap::from([("storage".into(), "riesel.storage.memory".into())]);
        assert!(matches!(
            registry.resolve(&app(), &selection),
            Err(ResolutionError::IncompatibleProvider { version: 1, .. })
        ));
    }

    #[test]
    fn rejects_ambiguous_matches_and_allows_explicit_selection() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(provider("riesel.storage.memory", 1))
            .unwrap();
        registry
            .register(provider("riesel.storage.disk", 1))
            .unwrap();

        assert!(matches!(
            registry.resolve(&app(), &BTreeMap::new()),
            Err(ResolutionError::AmbiguousProvider { .. })
        ));

        let selection = BTreeMap::from([("storage".into(), "riesel.storage.memory".into())]);
        let resolved = registry.resolve(&app(), &selection).unwrap();
        assert_eq!(resolved[0].provider, "riesel.storage.memory");
    }

    #[test]
    fn rejects_unknown_provider_and_binding() {
        let registry = ProviderRegistry::new();
        let selection = BTreeMap::from([("storage".into(), "riesel.storage.unknown".into())]);
        assert!(matches!(
            registry.resolve(&app(), &selection),
            Err(ResolutionError::UnknownProvider { .. })
        ));

        let selection = BTreeMap::from([("cache".into(), "riesel.storage.memory".into())]);
        assert!(matches!(
            registry.resolve(&app(), &selection),
            Err(ResolutionError::UnknownBinding(_))
        ));
    }

    #[test]
    fn rejects_duplicate_provider_ids() {
        let mut registry = ProviderRegistry::new();
        registry
            .register(provider("riesel.storage.memory", 1))
            .unwrap();
        assert!(matches!(
            registry.register(provider("riesel.storage.memory", 1)),
            Err(ResolutionError::DuplicateProvider(_))
        ));
    }
}
