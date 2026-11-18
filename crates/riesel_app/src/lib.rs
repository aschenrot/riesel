pub mod app;
mod component;
mod deployment;
mod plan;
mod recovery;
mod resolver;
mod runtime;
mod supervisor;
mod tasks;

pub use app::{App, AppError};
pub use component::{ComponentDefinition, DeclarativeComponent};
pub use deployment::{DeploymentConfig, DeploymentError, ServiceEndpoint};
pub use resolver::{InstanceIdentity, ProviderRegistry, ResolutionError, ResolvedCapability};

pub use runtime::{
    AppRunError, CapabilityConnector, ConnectedProvider, ConnectorError, ConnectorRegistry,
    LocalCapabilityBinding, LocalComponentFactory, LocalProviderFactory, LocalProviderInstance,
    RunningApp, RuntimeError, SingleLocalProvider, local_provider,
};

pub use supervisor::CancellationToken;

pub use plan::{DeploymentPlan, PlannedService, PlannedTarget};

pub use recovery::RecoveryPolicy;

pub use riesel_protocol::SessionToken;
