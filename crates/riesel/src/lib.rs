//! Riesel's Rust-first application SDK.
//!
//! Invalid Serde field modifiers must fail at compile time, rather than
//! silently producing RPC metadata inconsistent with the wire representation:
//!
//! ```compile_fail
//! #[derive(serde::Serialize, riesel::RieselType)]
//! struct Incorrect {
//!     #[serde(rename = "different")]
//!     original: String,
//! }
//! ```
//!
//! Native async capabilities are supported. To keep dispatch semantics
//! predictable, a single contract must not mix sync and async operations:
//!
//! ```compile_fail
//! use riesel::capability;
//! #[capability(id = "example.invalid", version = 1)]
//! trait Invalid {
//!     /// Read a counter asynchronously.
//!     #[query]
//!     async fn value(&self) -> Result<u64, String>;
//!
//!     /// Add a value synchronously.
//!     #[command]
//!     fn add(&self, amount: u64) -> Result<u64, String>;
//! }
//! ```
//! Riesel - a general purpose application engine
extern crate self as riesel;
mod application;
#[cfg(feature = "records")]
mod records;
mod rpc_connector;

pub use application::{
    Application, ApplicationHandler, ApplicationParam, Capability, Context, HandlerParams,
    PreparedApplication, Result, SdkError,
};
#[cfg(feature = "async-tcp")]
pub use application::{AsyncApplicationHandler, AsyncCapability, PreparedAsyncApplication};
#[cfg(feature = "records")]
pub use records::{Record, RecordPage, Records};
#[cfg(feature = "async-tcp")]
pub use riesel_app::{AsyncTasks, ProviderBlocking, ProviderBlockingError, ProviderCancellation};
#[cfg(feature = "async-tcp")]
pub use rpc_connector::AsyncRpcConnector;
pub use rpc_connector::RpcConnector;

pub use riesel_app::{
    App, AppError, AppRunError, CancellationToken, CapabilityConnector, CatalogError,
    ConnectedProvider, ConnectorError, ConnectorRegistry, DeclarativeComponent, DeploymentConfig,
    DeploymentError, LocalProviderFactory, ProviderCatalog, ProviderRegistry,
    ProviderSelectionConfig, RecoveryPolicy, ResolutionError, ResolvedCapability, RunningApp,
    RuntimeError, ServiceEndpoint, SessionToken,
};
pub use riesel_macros::{RieselModel, RieselType, app, capability, component};

/// An application entrypoint result that retains typed source errors behind
/// the existing structured ConnectorError boundary.
pub type AppResult<T = ()> = std::result::Result<T, riesel_app::ConnectorError>;
pub use riesel_schema::{CapabilityDescriptor, ModelField, RieselModel};

pub mod prelude {
    pub use super::{
        App, AppError, AppResult, Application, Capability, Result, RieselModel, app, capability,
        component,
    };
    #[cfg(feature = "async-tcp")]
    pub use super::{AsyncCapability, AsyncTasks};
    #[cfg(feature = "records")]
    pub use super::{Record, Records};
}
pub use riesel_schema as schema;
#[cfg(feature = "records")]
pub use riesel_storage as storage;
