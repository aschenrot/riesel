//! Riesel - a general purpose application engine
mod storage_connector;
pub use storage_connector::TcpRecordStoreConnector;

pub use riesel_app::{
    App, AppConfig, AppError, AppRunError, BindingError, CapabilityBindings, CapabilityConnector,
    ConnectedProvider, ConnectorError, ConnectorRegistry, DeploymentConfig, DeploymentError,
    ProviderRegistry, ResolutionError, ResolvedCapability, RunningApp, RuntimeError,
    ServiceEndpoint,
};
pub use riesel_macros::{RieselType, capability};

pub mod prelude {
    pub use super::{App, AppConfig, AppError};
}
pub use riesel_schema as schema;
pub use riesel_storage as storage;
