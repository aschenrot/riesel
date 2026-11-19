use crate::capability::valid_capability_id;
use crate::{CapabilityRef, CapabilityRefError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

/// Declarative identity of a provider and the exact contracts it implements.
/// This is metadata, not a factory or an instantiated service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDefinition {
    pub id: String,
    pub capabilities: Vec<CapabilityRef>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProviderError {
    #[error("invalid provider identifier: {0}")]
    InvalidId(String),
    #[error("provider {0} does not declare any capabilities")]
    NoCapabilities(String),
    #[error("invalid capability in provider {provider}: {source}")]
    InvalidCapability {
        provider: String,
        #[source]
        source: CapabilityRefError,
    },
    #[error("provider {provider} declares capability {id}@{version} more than once")]
    DuplicateCapability {
        provider: String,
        id: String,
        version: u32,
    },
}

impl ProviderDefinition {
    /// Compare published identity and capability sets without depending on
    /// the declaration order of independent capability contracts.
    pub fn has_same_definition(&self, other: &Self) -> bool {
        self.id == other.id
            && self.capabilities.len() == other.capabilities.len()
            && self
                .capabilities
                .iter()
                .all(|capability| other.capabilities.contains(capability))
    }

    pub fn validate(&self) -> Result<(), ProviderError> {
        if !valid_capability_id(&self.id) {
            return Err(ProviderError::InvalidId(self.id.clone()));
        }
        if self.capabilities.is_empty() {
            return Err(ProviderError::NoCapabilities(self.id.clone()));
        }
        let mut seen = BTreeSet::new();
        for capability in &self.capabilities {
            capability
                .validate()
                .map_err(|source| ProviderError::InvalidCapability {
                    provider: self.id.clone(),
                    source,
                })?;
            if !seen.insert((&capability.id, capability.version)) {
                return Err(ProviderError::DuplicateCapability {
                    provider: self.id.clone(),
                    id: capability.id.clone(),
                    version: capability.version,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn definition() -> ProviderDefinition {
        ProviderDefinition {
            id: "riesel.storage.memory".into(),
            capabilities: vec![CapabilityRef {
                id: "riesel.storage.records".into(),
                version: 1,
            }],
        }
    }

    #[test]
    fn validates_and_serializes_provider_definition() {
        let provider = definition();
        provider.validate().unwrap();
        let serialized = serde_json::to_string(&provider).unwrap();
        let restored: ProviderDefinition = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored, provider);
    }

    #[test]
    fn provider_identity_is_independent_of_capability_declaration_order() {
        let mut first = definition();
        first.capabilities.push(CapabilityRef {
            id: "riesel.storage.observer".into(),
            version: 1,
        });
        let mut reordered = first.clone();
        reordered.capabilities.reverse();
        assert!(first.has_same_definition(&reordered));
        reordered.capabilities[0].version = 2;
        assert!(!first.has_same_definition(&reordered));
    }

    #[test]
    fn rejects_duplicate_capability_contract() {
        let mut provider = definition();
        provider.capabilities.push(provider.capabilities[0].clone());
        assert!(matches!(
            provider.validate(),
            Err(ProviderError::DuplicateCapability { .. })
        ));
    }

    #[test]
    fn rejects_invalid_provider_and_empty_capabilities() {
        let mut provider = definition();
        provider.id = "MemoryStore".into();
        assert!(matches!(
            provider.validate(),
            Err(ProviderError::InvalidId(_))
        ));
        provider.id = "riesel.storage.memory".into();
        provider.capabilities.clear();
        assert!(matches!(
            provider.validate(),
            Err(ProviderError::NoCapabilities(_))
        ));
    }
}
