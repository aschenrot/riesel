//! Provider-agnostic application composition.
//! Connectors implement transport-specific discovery and typed service creation;
//! this module owns validation, resolution, and the resulting service lifetimes.

use crate::plan::{DeploymentPlan, PlannedService, PlannedTarget};
use crate::recovery::run_with_recovery;
use crate::supervisor::{CancellationToken, ServiceMonitor};
use crate::tasks::TaskGroup;
#[cfg(feature = "async-tcp")]
use crate::AsyncTasks;
use crate::{
    App, DeploymentConfig, DeploymentError, ProviderRegistry, ResolutionError, ResolvedCapability,
    ServiceEndpoint,
};
use riesel_manager::{ProcessKey, ProcessRegistry};
use riesel_protocol::SessionToken;
use riesel_schema::{CapabilityContract, ProviderDefinition};
use serde::Deserialize;
use std::any::Any;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;
use thiserror::Error;

pub type ConnectorError = Box<dyn Error + Send + Sync>;

mod errors;
mod managed;
mod planning;
mod registry;
mod running;
mod startup;
#[cfg(test)]
mod tests;

pub use errors::{AppRunError, RuntimeError};
pub use registry::{
    CapabilityConnector, ConnectedProvider, ConnectorRegistry, LocalCapabilityBinding,
    LocalComponentFactory, LocalProviderFactory, LocalProviderInstance, SingleLocalProvider,
    local_provider,
};
pub use running::RunningApp;
