pub mod app;
#[cfg(feature = "async-tcp")]
mod async_tasks;
mod catalog;
mod component;
mod deployment;
mod execution_plan;
#[cfg(feature = "async-tcp")]
mod execution_runner;
mod plan;
mod recovery;
mod resolver;
mod runtime;
mod supervisor;
mod tasks;

pub use app::{App, AppError};
#[cfg(feature = "async-tcp")]
pub use async_tasks::AsyncTasks;
pub use catalog::{CatalogError, ProviderCatalog, ProviderSelectionConfig};
pub use component::{ComponentDefinition, DeclarativeComponent};
pub use deployment::{DeploymentConfig, DeploymentError, ServiceEndpoint};
pub use execution_plan::{
    AccessMode, BoundExecutionPlan, ExecutionPlanDescription, ExecutionPlanDraft,
    ExecutionPlanError, OperationKey, OperationNode, OperationRegistry, OperationTrigger,
    PlanRevisionCatalog, ResourceAccess, ValidatedExecutionPlan,
};
#[cfg(feature = "async-tcp")]
pub use execution_runner::{
    AsyncOperation, ExecutionGraphRunner, GraphRunError, GraphRunReport, GraphSignalSender,
    OperationFuture, OperationOutcome, OperationState, async_operation,
};
pub use resolver::{InstanceIdentity, ProviderRegistry, ResolutionError, ResolvedCapability};
#[cfg(feature = "async-tcp")]
pub use riesel_protocol::{ProviderBlocking, ProviderBlockingError, ProviderCancellation};

pub use runtime::{
    AppRunError, CapabilityConnector, ConnectedProvider, ConnectorError, ConnectorRegistry,
    HookFailure, HookPhase, LifecycleFailure, LocalCapabilityBinding, LocalComponentFactory,
    LocalProviderFactory, LocalProviderInstance, RunningApp, RuntimeError, SingleLocalProvider,
    local_provider,
};

pub use supervisor::CancellationToken;

pub use plan::{DeploymentPlan, PlannedService, PlannedTarget};

pub use recovery::RecoveryPolicy;

pub use riesel_protocol::SessionToken;
