//! Riesel - a general purpose application engine
extern crate self as riesel;
mod application;
mod records;
mod storage_connector;

pub use application::{Application, Context, Result, SdkError};
pub use records::{Record, Records};
pub use storage_connector::TcpRecordStoreConnector;

pub use riesel_app::{
    App, AppConfig, AppError, AppRunError, BindingError, CapabilityBindings, CapabilityConnector,
    ConnectedProvider, ConnectorError, ConnectorRegistry, DeploymentConfig, DeploymentError,
    LocalProviderFactory, ProviderRegistry, ResolutionError, ResolvedCapability, RunningApp,
    RuntimeError, ServiceEndpoint,
};
pub use riesel_macros::{RieselModel, RieselType, capability};
pub use riesel_schema::{ModelField, RieselModel};

pub mod prelude {
    pub use super::{Application, Result, RieselModel};
}
pub use riesel_schema as schema;
pub use riesel_storage as storage;
