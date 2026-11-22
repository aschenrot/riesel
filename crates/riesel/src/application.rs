#[cfg(feature = "async-tcp")]
use crate::AsyncRpcConnector;
use crate::{CancellationToken, RpcConnector};
#[cfg(feature = "records")]
use crate::{Records, RieselModel};
use riesel_app::{
    App, AppRunError, ConnectorError, ConnectorRegistry, DeclarativeComponent, DeploymentConfig,
    DeploymentPlan, LocalProviderFactory, ProviderCatalog, ProviderSelectionConfig, RecoveryPolicy,
    RunningApp, RuntimeError,
};
#[cfg(feature = "async-tcp")]
use riesel_protocol::AsyncRpcClientBinding;
use riesel_protocol::RpcClientBinding;
use riesel_schema::{AppDefinition, AppSchema, CapabilityDescriptor, CapabilityRequirement};
#[cfg(feature = "records")]
use riesel_schema::{LinkedRecordConsistency, LinkedRecordQuery, OperationKind};
#[cfg(feature = "records")]
use riesel_storage::RecordStoreContract;
#[cfg(feature = "records")]
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use thiserror::Error;

#[cfg(feature = "async-tcp")]
mod async_handler;
mod builder;
mod cli;
mod context;
mod errors;
mod handler;
#[cfg(feature = "async-tcp")]
mod lifecycle;
mod module;
mod parameter;
#[cfg(test)]
mod tests;

#[cfg(feature = "async-tcp")]
pub use async_handler::{AsyncApplicationHandler, PreparedAsyncApplication};
pub use builder::Application;
pub use context::Context;
pub use errors::{Result, SdkError};
pub use handler::{ApplicationHandler, HandlerParams, PreparedApplication};
#[cfg(feature = "async-tcp")]
use lifecycle::LifecycleHooks;
pub use module::ApplicationModule;
pub use parameter::{ApplicationParam, Capability};
#[cfg(feature = "records")]
pub use parameter::ModelRead;
#[cfg(feature = "async-tcp")]
pub use parameter::{AsyncCapability, NativeHandleParam};
