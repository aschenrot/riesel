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
//! Generated capability macros also reject unsupported asynchronous methods:
//!
//! ```compile_fail
//! use riesel::capability;
//! #[capability(id = "example.invalid", version = 1)]
//! trait Invalid {
//!     /// Read a counter.
//!     #[query]
//!     async fn value(&self) -> Result<u64, String>;
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
#[cfg(feature = "records")]
pub use records::{Record, RecordPage, Records};
pub use rpc_connector::RpcConnector;

pub use riesel_app::{
    App, AppError, AppRunError, CancellationToken, CapabilityConnector, ConnectedProvider,
    ConnectorError, ConnectorRegistry, DeclarativeComponent, DeploymentConfig, DeploymentError,
    LocalProviderFactory, ProviderRegistry, ResolutionError, ResolvedCapability, RunningApp,
    RuntimeError, ServiceEndpoint, SessionToken,
};
pub use riesel_macros::{RieselModel, RieselType, capability, component};
pub use riesel_schema::{CapabilityDescriptor, ModelField, RieselModel};

pub mod prelude {
    pub use super::{App, AppError, Application, Capability, Result, RieselModel, capability, component};
    #[cfg(feature = "records")]
    pub use super::{Record, Records};
}
pub use riesel_schema as schema;
#[cfg(feature = "records")]
pub use riesel_storage as storage;
