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
//! Invalid high-level application signatures fail during macro expansion:
//!
//! ```compile_fail
//! use riesel::app;
//! #[app]
//! fn incorrect() -> riesel::AppResult<()> { Ok(()) }
//! ```
//!
//! Riesel - a general purpose application engine
extern crate self as riesel;
mod application;
#[cfg(feature = "records")]
mod records;
mod rpc_connector;

#[cfg(feature = "records")]
pub use application::ModelRead;
pub use application::{
    Application, ApplicationHandler, ApplicationModule, ApplicationParam, Capability, Context,
    HandlerParams, PreparedApplication, Result, SdkError,
};
#[cfg(feature = "async-tcp")]
pub use application::{
    AsyncApplicationHandler, AsyncCapability, NativeHandleParam, PreparedAsyncApplication,
};
#[cfg(feature = "records")]
pub use records::{QueryPage, Record, RecordPage, Records};
#[cfg(feature = "experimental-typed-dataflow")]
pub use riesel_app::{
    AsyncTypedOperation, LocalPort, LocalPortType, PreparedTypedExecution, TypedEdge,
    TypedExecutionGraphRunner, TypedExecutionPlan, TypedGraphError, TypedGraphRunReport,
    TypedInputs, TypedOperationFuture, TypedOperationOutcome, TypedOperationRegistry,
    TypedOperationSpec, TypedOutputs, typed_operation,
};
#[cfg(feature = "async-tcp")]
pub use riesel_app::{
    AsyncOperation, ExecutionGraphRunner, GraphControlError, GraphControlMode, GraphControlTarget,
    GraphEffectError, GraphEffectRecorder, GraphExecutionFailure, GraphObservationError,
    GraphObservedOperation, GraphRunChange, GraphRunController, GraphRunError, GraphRunIdentity,
    GraphRunObserver, GraphRunPhase, GraphRunReport, GraphRunSnapshot, GraphRunTransition,
    GraphRunTransitions, GraphSignalSender, MAX_GRAPH_EFFECT_STREAMS, MAX_GRAPH_TRANSITIONS,
    MAX_NESTED_TRANSITIONS, NestedCursor, NestedDescription, NestedDomainOwner,
    NestedDomainRegistration, NestedHeaderObserver, NestedObservationError, NestedObserver,
    NestedParent, NestedPublisher, NestedSnapshot, NestedStatus, NestedTransitions, NestedViewInfo,
    OperationFuture, OperationOutcome, OperationState, async_operation,
    async_operation_with_effects,
};
#[cfg(feature = "async-tcp")]
pub use riesel_app::{AsyncTasks, ProviderBlocking, ProviderBlockingError, ProviderCancellation};
#[cfg(feature = "async-tcp")]
pub use rpc_connector::AsyncRpcConnector;
pub use rpc_connector::RpcConnector;

#[cfg(feature = "internal-publication")]
#[doc(hidden)]
pub use riesel_app::InternalEntrypointPhase;
/// Internal, opt-in research adapter only; no stable observer/export API.
#[cfg(feature = "internal-publication")]
#[doc(hidden)]
pub use riesel_app::operational_observation as internal_observation;
pub use riesel_app::{
    AccessMode, App, AppError, AppRunError, BoundExecutionPlan, CancellationToken,
    CapabilityConnector, CatalogError, ConnectedProvider, ConnectorError, ConnectorRegistry,
    DeclarativeComponent, DeploymentConfig, DeploymentError, DomainAdmissionMode, DomainBoundary,
    EditorAffinity, EditorCatalogError, EditorOperationDescriptor, ExecutionPlanDescription,
    ExecutionPlanDraft, ExecutionPlanError, FixedStepAdmission, FixedStepBacklog, FixedStepError,
    FixedStepFrame, FixedStepPolicy, HookFailure, HookPhase, LifecycleFailure,
    LocalProviderFactory, MAX_FIXED_TICK_BACKLOG, MAX_FIXED_TICKS_PER_FRAME, OperationKey,
    OperationNode, OperationRegistry, OperationTrigger, PlanRevisionCatalog, ProviderCatalog,
    ProviderRegistry, ProviderSelectionConfig, RecoveryPolicy, ResolutionError, ResolvedCapability,
    ResourceAccess, RunningApp, RuntimeError, ServiceEndpoint, SessionToken,
    ValidatedExecutionPlan,
};
pub use riesel_app::{
    S2_DOCUMENT_VERSION, S2DocumentAccess, S2DocumentAccessMode, S2DocumentDomain,
    S2DocumentDomainMode, S2DocumentError, S2DocumentLimits, S2DocumentTrigger, S2OperationBinding,
    S2OperationDocument, S2PlanDocument, S2SourceDocument, bind_s2_document, inspect_s2_source,
};

pub use riesel_app::{
    CheckedSourceHandoff, MAX_STORED_LAYOUT_BYTES, MAX_STORED_PLAN_BYTES, PlanAuthoringHead,
    PlanAuthoringSnapshot, PlanAuthoringSource, PlanLayoutDraft, PlanLayoutPoint,
    PlanRevisionError, PlanRevisionRepository, PlanSemanticCandidate, StoredPlanLayout,
    StoredSemanticPlan, check_layout_nodes,
};

pub use riesel_app::{PlanSourceCoverageReport, SourceCoverageFinding, SourceCoverageProblem};

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
    pub use super::{ModelRead, QueryPage, Record, Records};
}
pub use riesel_schema as schema;
#[cfg(feature = "records")]
pub use riesel_storage as storage;
