use riesel_schema::{AppSchema, SchemaError};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::Path;
use thiserror::Error;

/// Connection settings are deployment-specific, not part of a capability contract.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentConfig {
    pub services: BTreeMap<String, ServiceEndpoint>,
}

/// Transport for an externally started service.
#[derive(Debug, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServiceEndpoint {
    Tcp { address: SocketAddr },
}

#[derive(Debug, Error)]
pub enum DeploymentError {
    #[error("cannot read deployment configuration: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid deployment JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid application schema: {0}")]
    Schema(#[from] SchemaError),
    #[error("deployment specifies unknown requirement: {0}")]
    UnknownRequirement(String),
    #[error("deployment does not configure required service: {0}")]
    MissingService(String),
}

impl DeploymentConfig {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DeploymentError> {
        Self::from_json(&std::fs::read_to_string(path)?)
    }

    pub fn from_json(source: &str) -> Result<Self, DeploymentError> {
        Ok(serde_json::from_str(source)?)
    }

    /// Validate that all declared requirements have one configured endpoint,
    /// with no stale or unknown configuration keys.
    pub fn validate(&self, app: &AppSchema) -> Result<(), DeploymentError> {
        app.validate()?;
        for name in self.services.keys() {
            if !app.requires.iter().any(|requirement| requirement.name == *name) {
                return Err(DeploymentError::UnknownRequirement(name.clone()));
            }
        }
        for requirement in &app.requires {
            if !self.services.contains_key(&requirement.name) {
                return Err(DeploymentError::MissingService(requirement.name.clone()));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::parse_schema;

    fn app() -> AppSchema {
        parse_schema(r#"{
            "version": 1,
            "app": {"name": "Todo"},
            "models": {},
            "requires": [{"name": "storage", "capability": {
                "id": "riesel.storage.records", "version": 1
            }}]
        }"#).unwrap()
    }

    #[test]
    fn parses_valid_tcp_binding() {
        let deployment = DeploymentConfig::from_json(
            r#"{"services":{"storage":{"transport":"tcp","address":"127.0.0.1:47911"}}}"#
        ).unwrap();
        deployment.validate(&app()).unwrap();
        assert!(matches!(
            deployment.services["storage"],
            ServiceEndpoint::Tcp { .. }
        ));
    }

    #[test]
    fn rejects_missing_or_unknown_bindings() {
        let missing = DeploymentConfig::from_json(r#"{"services":{}}"#).unwrap();
        assert!(matches!(
            missing.validate(&app()),
            Err(DeploymentError::MissingService(name)) if name == "storage"
        ));

        let unknown = DeploymentConfig::from_json(
            r#"{"services":{"other":{"transport":"tcp","address":"127.0.0.1:47911"}}}"#
        ).unwrap();
        assert!(matches!(
            unknown.validate(&app()),
            Err(DeploymentError::UnknownRequirement(name)) if name == "other"
        ));
    }

    #[test]
    fn rejects_invalid_addresses_and_unknown_fields() {
        assert!(DeploymentConfig::from_json(
            r#"{"services":{"storage":{"transport":"tcp","address":"not an address"}}}"#
        ).is_err());
        assert!(DeploymentConfig::from_json(
            r#"{"services":{"storage":{"transport":"tcp","address":"127.0.0.1:47911","extra":1}}}"#
        ).is_err());
    }
}
