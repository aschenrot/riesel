pub mod app;
pub mod config;
mod deployment;
mod resolver;
mod runtime;

pub use app::{App, AppError};
pub use config::AppConfig;
pub use deployment::{DeploymentConfig, DeploymentError, ServiceEndpoint};
pub use resolver::{ProviderRegistry, ResolutionError, ResolvedCapability};

pub use runtime::{
    AppRunError, CapabilityConnector, ConnectedProvider, ConnectorError, ConnectorRegistry,
    LocalProviderFactory, RunningApp, RuntimeError,
};
