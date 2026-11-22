pub mod app;
#[cfg(feature = "async-tcp")]
mod async_tasks;
mod catalog;
mod component;
mod deployment;
mod editor_catalog;
#[cfg(feature = "async-tcp")]
mod execution_nested;
#[cfg(feature = "async-tcp")]
mod execution_observation;
mod execution_plan;
#[cfg(feature = "async-tcp")]
mod execution_runner;
mod fixed_step;
#[cfg(all(test, feature = "internal-publication"))]
mod internal_publication_tests;
#[cfg(feature = "internal-publication")]
#[doc(hidden)]
pub mod operational_observation;
mod plan;
mod plan_revision;
mod recovery;
mod resolver;
mod runtime;
mod s2_document;
mod supervisor;
mod tasks;

pub use app::{App, AppError};
#[cfg(feature = "async-tcp")]
pub use async_tasks::AsyncTasks;
#[cfg(feature = "internal-publication")]
#[doc(hidden)]
pub use async_tasks::{
    InternalEntrypointActivity, InternalEntrypointGuard, InternalEntrypointPhase,
};
pub use catalog::{CatalogError, ProviderCatalog, ProviderSelectionConfig};
pub use component::{ComponentDefinition, DeclarativeComponent};
pub use deployment::{DeploymentConfig, DeploymentError, ServiceEndpoint};
pub use editor_catalog::{EditorAffinity, EditorCatalogError, EditorOperationDescriptor};
#[cfg(feature = "async-tcp")]
pub use execution_nested::{
    MAX_NESTED_TRANSITIONS, NestedCursor, NestedDescription, NestedDomainOwner,
    NestedDomainRegistration, NestedHeaderObserver, NestedObservationError, NestedObserver,
    NestedParent, NestedPublisher, NestedSnapshot, NestedStatus, NestedTransitions, NestedViewInfo,
};
#[cfg(feature = "async-tcp")]
pub use execution_observation::{
    GraphControlError, GraphControlMode, GraphControlTarget, GraphEffectError, GraphEffectRecorder,
    GraphObservationError, GraphObservedOperation, GraphRunChange, GraphRunController,
    GraphRunIdentity, GraphRunObserver, GraphRunPhase, GraphRunSnapshot, GraphRunTransition,
    GraphRunTransitions, MAX_GRAPH_EFFECT_STREAMS, MAX_GRAPH_TRANSITIONS,
};
pub use execution_plan::{
    AccessMode, BoundExecutionPlan, DomainAdmissionMode, DomainBoundary, ExecutionPlanDescription,
    ExecutionPlanDraft, ExecutionPlanError, OperationKey, OperationNode, OperationRegistry,
    OperationTrigger, PlanRevisionCatalog, ResourceAccess, ValidatedExecutionPlan,
};
#[cfg(feature = "async-tcp")]
pub use execution_runner::{
    AsyncOperation, ExecutionGraphRunner, GraphExecutionFailure, GraphRunError, GraphRunReport,
    GraphSignalSender, OperationFuture, OperationOutcome, OperationState, async_operation,
    async_operation_with_effects,
};
pub use fixed_step::{
    FixedStepAdmission, FixedStepBacklog, FixedStepError, FixedStepFrame, FixedStepPolicy,
    MAX_FIXED_TICK_BACKLOG, MAX_FIXED_TICKS_PER_FRAME,
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
pub use plan_revision::{
    MAX_STORED_LAYOUT_BYTES, MAX_STORED_PLAN_BYTES, CheckedSourceHandoff, PlanAuthoringHead,
    PlanAuthoringSource, PlanLayoutDraft, PlanLayoutPoint, PlanRevisionError,
    PlanRevisionRepository, PlanSemanticCandidate, StoredPlanLayout, StoredSemanticPlan,
    check_layout_nodes,
};

pub use recovery::RecoveryPolicy;
pub use s2_document::{
    S2_DOCUMENT_VERSION, S2DocumentAccess, S2DocumentAccessMode, S2DocumentDomain,
    S2DocumentDomainMode, S2DocumentError, S2DocumentLimits, S2DocumentTrigger, S2OperationBinding,
    S2OperationDocument, S2PlanDocument, S2SourceDocument, bind_s2_document, inspect_s2_source,
};

pub use riesel_protocol::SessionToken;
