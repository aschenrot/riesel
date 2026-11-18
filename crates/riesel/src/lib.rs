//! Riesel - a general purpose application engine
mod storage_connector;
pub use storage_connector::TcpRecordStoreConnector;

pub use riesel_app::{
    App, AppConfig, AppError, BindingError, CapabilityBindings, DeploymentConfig, DeploymentError,
    ProviderRegistry, ResolutionError, ResolvedCapability, ServiceEndpoint,
    AppRunError, CapabilityConnector, ConnectedProvider, ConnectorError, ConnectorRegistry, RunningApp, RuntimeError,
};
pub use riesel_macros::{RieselType, capability};

pub mod prelude {
    pub use super::{App, AppConfig, AppError};
}
pub use riesel_schema as schema;
pub use riesel_storage as storage;
