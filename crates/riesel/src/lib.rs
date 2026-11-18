//! Riesel - a general purpose application engine
extern crate self as riesel;
mod application;
mod records;
mod rpc_connector;

pub use application::{Application, Context, Result, SdkError};
pub use records::{Record, Records};
pub use rpc_connector::RpcConnector;

pub use riesel_app::{
    App, AppConfig, AppError, AppRunError, CapabilityConnector, ConnectedProvider, ConnectorError,
    ConnectorRegistry, DeploymentConfig, DeploymentError, LocalProviderFactory, ProviderRegistry,
    ResolutionError, ResolvedCapability, RunningApp, RuntimeError, ServiceEndpoint,
};
pub use riesel_macros::{RieselModel, RieselType, capability};
pub use riesel_schema::{CapabilityDescriptor, ModelField, RieselModel};

pub mod prelude {
    pub use super::{App, AppConfig, AppError, Application, Result, RieselModel};
}
pub use riesel_schema as schema;
pub use riesel_storage as storage;
