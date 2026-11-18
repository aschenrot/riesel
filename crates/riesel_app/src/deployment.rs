use riesel_schema::{AppSchema, SchemaError};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Connection settings are deployment-specific, not part of a capability contract.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentConfig {
    pub services: BTreeMap<String, ServiceEndpoint>,
}

/// Deployment-level connection and optional process ownership. Execution
/// configuration stays out of the application capability contract.
#[derive(Debug, Deserialize)]
#[serde(tag = "transport", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServiceEndpoint {
    /// The service is independently started and owned by its operator.
    Tcp { address: SocketAddr },
    /// In-process provider selected by ID; no transport or process is required.
    Local { provider: String },
    /// Managed loopback service that chooses its own port at bind time.
    /// The provider must implement the RIESEL_READY startup announcement.
    ManagedTcpDynamic {
        program: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default = "default_startup_timeout_ms")]
        startup_timeout_ms: u64,
    },
    /// Riesel owns the child process and terminates it on shutdown.
    /// The deployment file must be trusted; its program is executable code.
    ManagedTcp {
        address: SocketAddr,
        program: PathBuf,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default = "default_startup_timeout_ms")]
        startup_timeout_ms: u64,
    },
}

fn default_startup_timeout_ms() -> u64 {
    10_000
}

impl ServiceEndpoint {
    pub fn address(&self) -> Option<SocketAddr> {
        match self {
            Self::Tcp { address } | Self::ManagedTcp { address, .. } => Some(*address),
            Self::Local { .. } | Self::ManagedTcpDynamic { .. } => None,
        }
    }
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
    #[error("invalid endpoint for requirement {name}: {reason}")]
    InvalidEndpoint { name: String, reason: &'static str },
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
            if !app
                .requires
                .iter()
                .any(|requirement| requirement.name == *name)
            {
                return Err(DeploymentError::UnknownRequirement(name.clone()));
            }
        }
        for requirement in &app.requires {
            if !self.services.contains_key(&requirement.name) {
                return Err(DeploymentError::MissingService(requirement.name.clone()));
            }
        }
        for (name, endpoint) in &self.services {
            if endpoint
                .address()
                .is_some_and(|address| address.port() == 0)
            {
                return Err(DeploymentError::InvalidEndpoint {
                    name: name.clone(),
                    reason: "port zero is not a valid client endpoint",
                });
            }
            if let ServiceEndpoint::ManagedTcpDynamic {
                program,
                startup_timeout_ms,
                ..
            } = endpoint
            {
                if program.as_os_str().is_empty() || !(1..=120_000).contains(startup_timeout_ms) {
                    return Err(DeploymentError::InvalidEndpoint {
                        name: name.clone(),
                        reason: "dynamic managed service requires a program and a valid timeout",
                    });
                }
            }
            if let ServiceEndpoint::ManagedTcp {
                address,
                program,
                startup_timeout_ms,
                ..
            } = endpoint
            {
                if !address.ip().is_loopback() {
                    return Err(DeploymentError::InvalidEndpoint {
                        name: name.clone(),
                        reason: "managed unauthenticated services must use a loopback address",
                    });
                }
                if program.as_os_str().is_empty() {
                    return Err(DeploymentError::InvalidEndpoint {
                        name: name.clone(),
                        reason: "managed program path cannot be empty",
                    });
                }
                if !(1..=120_000).contains(startup_timeout_ms) {
                    return Err(DeploymentError::InvalidEndpoint {
                        name: name.clone(),
                        reason: "startup timeout must be between 1 and 120000 ms",
                    });
                }
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
        parse_schema(
            r#"{
            "version": 1,
            "app": {"name": "Todo"},
            "models": {},
            "requires": [{"name": "storage", "capability": {
                "id": "riesel.storage.records", "version": 1
            }}]
        }"#,
        )
        .unwrap()
    }

    #[test]
    fn parses_valid_tcp_binding() {
        let deployment = DeploymentConfig::from_json(
            r#"{"services":{"storage":{"transport":"tcp","address":"127.0.0.1:47911"}}}"#,
        )
        .unwrap();
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
            r#"{"services":{"other":{"transport":"tcp","address":"127.0.0.1:47911"}}}"#,
        )
        .unwrap();
        assert!(matches!(
            unknown.validate(&app()),
            Err(DeploymentError::UnknownRequirement(name)) if name == "other"
        ));
    }

    #[test]
    fn rejects_invalid_addresses_and_unknown_fields() {
        assert!(
            DeploymentConfig::from_json(
                r#"{"services":{"storage":{"transport":"tcp","address":"not an address"}}}"#
            )
            .is_err()
        );
        assert!(DeploymentConfig::from_json(
            r#"{"services":{"storage":{"transport":"tcp","address":"127.0.0.1:47911","extra":1}}}"#
        ).is_err());
    }
    #[test]
    fn validates_trusted_managed_process_settings() {
        let source = r#"{"services":{"storage":{
            "transport":"managed_tcp",
            "address":"127.0.0.1:47911",
            "program":"storage_service",
            "args":["127.0.0.1:47911"]
        }}}"#;
        let config = DeploymentConfig::from_json(source).unwrap();
        config.validate(&app()).unwrap();
        assert!(matches!(
            &config.services["storage"],
            ServiceEndpoint::ManagedTcp {
                startup_timeout_ms: 10_000,
                ..
            }
        ));
    }

    #[test]
    fn rejects_dangerous_or_invalid_managed_endpoints() {
        for (address, program, timeout) in [
            ("0.0.0.0:47911", "service", 1000),
            ("127.0.0.1:0", "service", 1000),
            ("127.0.0.1:47911", "", 1000),
            ("127.0.0.1:47911", "service", 0),
            ("127.0.0.1:47911", "service", 120001),
        ] {
            let value = serde_json::json!({
                "services": {
                    "storage": {
                        "transport": "managed_tcp",
                        "address": address,
                        "program": program,
                        "startup_timeout_ms": timeout
                    }
                }
            });
            let config = DeploymentConfig::from_json(&value.to_string()).unwrap();
            assert!(matches!(
                config.validate(&app()),
                Err(DeploymentError::InvalidEndpoint { .. })
            ));
        }
    }
    #[test]
    fn dynamic_managed_service_requires_no_port() {
        let source = r#"{"services":{"storage":{
            "transport":"managed_tcp_dynamic",
            "program":"storage_service",
            "startup_timeout_ms":3000
        }}}"#;
        let config = DeploymentConfig::from_json(source).unwrap();
        config.validate(&app()).unwrap();
        assert!(config.services["storage"].address().is_none());
    }

}
