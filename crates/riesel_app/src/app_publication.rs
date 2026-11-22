//! Opt-in application-owned, local read-only publication admission.
//!
//! This module is the transport-neutral policy core for #258, not a listener,
//! an endpoint identity verifier or a protected individual-client credential.
//! Its T1 caller profile explicitly trusts processes running as the same OS
//! user. Adapters must enforce their declared local/browser-origin boundary.
use crate::RunningApp;
use riesel_protocol::{
    AsyncRpcHandler, RpcError, SessionToken,
    session::{InternalAdmissionError, InternalPublicationSession},
};
use riesel_schema::{CapabilityContract, CapabilityRef, OperationKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, PoisonError};
use thiserror::Error;

/// Explicit application declaration. Shared-user peers are trusted by T1;
/// this does not authorize private data, mutations or Manager operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadExposure {
    NonSensitiveSharedUser,
}

/// Public, non-secret description. It cannot authenticate the app or a client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadPublicationDescriptor {
    pub application: String,
    pub interface: String,
    pub contract: CapabilityRef,
    pub generation: String,
    pub allowed_queries: Vec<String>,
    pub exposure: ReadExposure,
}

/// Fail closed before dispatch for invalid scope or unsafe operation kinds.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReadPublicationError {
    #[error("invalid deliberate read publication: {0}")]
    Invalid(String),
    #[error("selected operation is not a read-only query: {0}")]
    NotReadOnly(String),
    #[error("application or interface does not match the selected publication")]
    WrongInterface,
    #[error("versioned capability contract does not match the selected publication")]
    WrongContract,
    #[error("application runtime generation is stale")]
    StaleGeneration,
    #[error("publication was withdrawn")]
    Withdrawn,
    #[error("operation was not explicitly published")]
    NotPublished,
    #[error("runtime admission capacity reached")]
    AtCapacity,
    #[error("application runtime session expired")]
    Expired,
    #[error("request or response exceeded the bounded read budget")]
    OverBudget,
    #[error(transparent)]
    Rpc(#[from] RpcError),
}

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const MAX_SELECTED_QUERIES: usize = 128;
const MAX_PUBLIC_ID_BYTES: usize = 128;

#[derive(Debug)]
struct PublicationPolicy {
    withdrawn: bool,
}

/// A selected, versioned application-facing query surface tied to exactly
/// one real Riesel runtime session. Does not own the app, graph, or GUI host.
pub struct AppReadPublication {
    descriptor: ReadPublicationDescriptor,
    allowed: BTreeSet<String>,
    contract: CapabilityContract,
    handler: Arc<dyn AsyncRpcHandler>,
    session: SessionToken,
    runtime_ingress: InternalPublicationSession,
    policy: Mutex<PublicationPolicy>,
}

impl AppReadPublication {
    fn from_running(
        running: &RunningApp,
        application: String,
        interface: String,
        exposure: ReadExposure,
        handler: Arc<dyn AsyncRpcHandler>,
        selected_queries: impl IntoIterator<Item = String>,
    ) -> Result<Self, ReadPublicationError> {
        for field in [&application, &interface] {
            if field.is_empty() || field.len() > MAX_PUBLIC_ID_BYTES || field.trim() != field {
                return Err(ReadPublicationError::Invalid(
                    "application/interface name absent, oversized or not normalized".into(),
                ));
            }
        }
        // Only a real RunningApp owns this runtime-issued nonsecret generation.
        // The public constructor does not accept a caller-provided SessionToken.
        let session = running.session_token();
        if !session.is_active() {
            return Err(ReadPublicationError::Expired);
        }
        handler.validate_host()?;
        let contract = handler.contract()?;
        let allowed: BTreeSet<String> = selected_queries.into_iter().collect();
        if allowed.is_empty() || allowed.len() > MAX_SELECTED_QUERIES {
            return Err(ReadPublicationError::Invalid(
                "selected query set must be nonempty and bounded".into(),
            ));
        }
        for query in &allowed {
            let spec = contract
                .operations
                .get(query)
                .ok_or_else(|| ReadPublicationError::Invalid(format!("unknown query: {query}")))?;
            if spec.kind != OperationKind::Query {
                return Err(ReadPublicationError::NotReadOnly(query.clone()));
            }
        }
        let runtime_ingress = session.internal_publication_session();
        let descriptor = ReadPublicationDescriptor {
            application,
            interface,
            contract: contract.capability.clone(),
            generation: runtime_ingress.generation(),
            allowed_queries: allowed.iter().cloned().collect(),
            exposure,
        };
        Ok(Self {
            descriptor,
            allowed,
            contract,
            handler,
            session,
            runtime_ingress,
            policy: Mutex::new(PublicationPolicy { withdrawn: false }),
        })
    }

    /// Advertised values are correlation hints, not a verified server identity
    /// or a secret. Do not infer client authorization from knowing them.
    pub fn descriptor(&self) -> ReadPublicationDescriptor {
        self.descriptor.clone()
    }

    /// Order app withdrawal with new admission. Already admitted reads may
    /// finish; a blocking mutex is never held across a user future.
    pub fn withdraw(&self) {
        self.policy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .withdrawn = true;
    }

    /// The adapter must first enforce local T1 scope. This method validates
    /// the exact operation and real runtime epoch, then holds the permit
    /// across the read. No transport is installed by this object.
    pub async fn query(
        &self,
        requested: &ReadPublicationDescriptor,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, ReadPublicationError> {
        if requested.application != self.descriptor.application
            || requested.interface != self.descriptor.interface
            || requested.exposure != self.descriptor.exposure
        {
            return Err(ReadPublicationError::WrongInterface);
        }
        if requested.contract != self.descriptor.contract {
            return Err(ReadPublicationError::WrongContract);
        }
        if requested.generation != self.descriptor.generation {
            return Err(ReadPublicationError::StaleGeneration);
        }
        if !self.allowed.contains(operation)
            || requested.allowed_queries != self.descriptor.allowed_queries
        {
            return Err(ReadPublicationError::NotPublished);
        }
        if serde_json::to_vec(&arguments)
            .map_err(|error| ReadPublicationError::Invalid(error.to_string()))?
            .len()
            > MAX_REQUEST_BYTES
        {
            return Err(ReadPublicationError::OverBudget);
        }

        // Publication withdrawal and Riesel runtime admission have one
        // pre-dispatch linearization point. No lock spans application code.
        let policy = self.policy.lock().unwrap_or_else(PoisonError::into_inner);
        if policy.withdrawn {
            return Err(ReadPublicationError::Withdrawn);
        }
        let permit = self
            .runtime_ingress
            .try_admit(&requested.generation)
            .map_err(|error| match error {
                InternalAdmissionError::WrongGeneration => ReadPublicationError::StaleGeneration,
                InternalAdmissionError::Expired => ReadPublicationError::Expired,
                InternalAdmissionError::AtCapacity => ReadPublicationError::AtCapacity,
            })?;
        drop(policy);

        let answer = self
            .handler
            .dispatch_for(&self.contract.capability, operation, arguments)
            .await;
        // A result from a closed runtime cannot be represented as fresh,
        // even if its async body completed after revocation.
        if !self.session.is_active() {
            return Err(ReadPublicationError::Expired);
        }
        let answer = answer?;
        if serde_json::to_vec(&answer)
            .map_err(|error| ReadPublicationError::Invalid(error.to_string()))?
            .len()
            > MAX_RESPONSE_BYTES
        {
            return Err(ReadPublicationError::OverBudget);
        }
        drop(permit);
        Ok(answer)
    }
}

impl RunningApp {
    /// Publish an opt-in app-facing T1 read-only interface. This does not
    /// create a listener, credential, process owner or graphical window.
    ///
    /// The app author must attest that selected queries contain no private
    /// data. Every external adapter must independently enforce its declared
    /// local and browser-origin access policy.
    pub fn publish_local_reads(
        &self,
        application: impl Into<String>,
        interface: impl Into<String>,
        exposure: ReadExposure,
        handler: Arc<dyn AsyncRpcHandler>,
        selected_queries: impl IntoIterator<Item = String>,
    ) -> Result<AppReadPublication, ReadPublicationError> {
        AppReadPublication::from_running(
            self,
            application.into(),
            interface.into(),
            exposure,
            handler,
            selected_queries,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{App, ConnectorRegistry};
    use riesel_protocol::AsyncRpcFuture;
    use riesel_schema::{OperationDefinition, TypeRef};
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SCHEMA: &str =
        r#"{"version":1,"app":{"name":"PublishedReadTests"},"models":{},"requires":[]}"#;

    struct Domain {
        reads: AtomicUsize,
        commands: AtomicUsize,
    }

    impl Domain {
        fn new() -> Self {
            Self {
                reads: AtomicUsize::new(0),
                commands: AtomicUsize::new(0),
            }
        }
    }

    impl AsyncRpcHandler for Domain {
        fn contract(&self) -> Result<CapabilityContract, RpcError> {
            Ok(CapabilityContract {
                capability: CapabilityRef {
                    id: "research.app_public_read".into(),
                    version: 1,
                },
                operations: BTreeMap::from([
                    (
                        "count".into(),
                        OperationDefinition {
                            kind: OperationKind::Query,
                            description: "Read an app-owned counter".into(),
                            inputs: Vec::new(),
                            output: TypeRef::U64,
                            error: TypeRef::Unit,
                        },
                    ),
                    (
                        "update".into(),
                        OperationDefinition {
                            kind: OperationKind::Command,
                            description: "Mutate the app-owned counter".into(),
                            inputs: Vec::new(),
                            output: TypeRef::U64,
                            error: TypeRef::Unit,
                        },
                    ),
                ]),
                types: BTreeMap::new(),
            })
        }

        fn dispatch<'a>(&'a self, operation: &'a str, _arguments: Value) -> AsyncRpcFuture<'a> {
            Box::pin(async move {
                match operation {
                    "count" => {
                        self.reads.fetch_add(1, Ordering::SeqCst);
                        Ok(json!(7))
                    }
                    "update" => {
                        self.commands.fetch_add(1, Ordering::SeqCst);
                        Ok(json!(8))
                    }
                    _ => Err(RpcError::UnknownOperation(operation.into())),
                }
            })
        }
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    fn app() -> RunningApp {
        App::from_schema(SCHEMA)
            .unwrap()
            .start_local(&ConnectorRegistry::new())
            .unwrap()
    }

    #[test]
    fn app_explicit_query_selection_never_exposes_internal_commands() {
        let mut running = app();
        let domain = Arc::new(Domain::new());

        let unsafe_publication = running.publish_local_reads(
            "research.app",
            "graph.visible",
            ReadExposure::NonSensitiveSharedUser,
            domain.clone(),
            ["count".into(), "update".into()],
        );
        assert!(matches!(
            unsafe_publication,
            Err(ReadPublicationError::NotReadOnly(name)) if name == "update"
        ));

        let publication = running
            .publish_local_reads(
                "research.app",
                "graph.visible",
                ReadExposure::NonSensitiveSharedUser,
                domain.clone(),
                ["count".into()],
            )
            .unwrap();
        let descriptor = publication.descriptor();
        assert_eq!(descriptor.contract.version, 1);
        assert_eq!(descriptor.exposure, ReadExposure::NonSensitiveSharedUser);
        assert_eq!(descriptor.allowed_queries, ["count"]);
        let mut wrong_version = descriptor.clone();
        wrong_version.contract.version += 1;
        let mut wrong_epoch = descriptor.clone();
        wrong_epoch.generation.push('x');
        let mut wrong_app = descriptor.clone();
        wrong_app.application.push('x');

        runtime().block_on(async {
            assert_eq!(
                publication
                    .query(&descriptor, "count", json!({}))
                    .await
                    .unwrap(),
                json!(7)
            );
            assert!(matches!(
                publication.query(&descriptor, "update", json!({})).await,
                Err(ReadPublicationError::NotPublished)
            ));
            assert!(matches!(
                publication.query(&wrong_version, "count", json!({})).await,
                Err(ReadPublicationError::WrongContract)
            ));
            assert!(matches!(
                publication.query(&wrong_epoch, "count", json!({})).await,
                Err(ReadPublicationError::StaleGeneration)
            ));
            assert!(matches!(
                publication.query(&wrong_app, "count", json!({})).await,
                Err(ReadPublicationError::WrongInterface)
            ));
            assert!(matches!(
                publication
                    .query(
                        &descriptor,
                        "count",
                        Value::String("x".repeat(MAX_REQUEST_BYTES + 1)),
                    )
                    .await,
                Err(ReadPublicationError::OverBudget)
            ));
            assert_eq!(domain.reads.load(Ordering::SeqCst), 1);
            assert_eq!(domain.commands.load(Ordering::SeqCst), 0);
            publication.withdraw();
            assert!(matches!(
                publication.query(&descriptor, "count", json!({})).await,
                Err(ReadPublicationError::Withdrawn)
            ));
        });
        running.shutdown().unwrap();
    }

    #[test]
    fn real_same_pid_runtime_replacement_fences_old_publication() {
        let mut old = app();
        let domain = Arc::new(Domain::new());
        let prior = old
            .publish_local_reads(
                "research.app",
                "graph.visible",
                ReadExposure::NonSensitiveSharedUser,
                domain.clone(),
                ["count".into()],
            )
            .unwrap();
        let stale = prior.descriptor();
        let pid = std::process::id();
        old.shutdown().unwrap();

        let mut next = app();
        assert_eq!(std::process::id(), pid);
        let successor = next
            .publish_local_reads(
                "research.app",
                "graph.visible",
                ReadExposure::NonSensitiveSharedUser,
                domain.clone(),
                ["count".into()],
            )
            .unwrap();
        let fresh = successor.descriptor();
        assert_ne!(stale.generation, fresh.generation);
        runtime().block_on(async {
            assert!(matches!(
                prior.query(&stale, "count", json!({})).await,
                Err(ReadPublicationError::Expired)
            ));
            assert!(matches!(
                successor.query(&stale, "count", json!({})).await,
                Err(ReadPublicationError::StaleGeneration)
            ));
            assert_eq!(
                successor.query(&fresh, "count", json!({})).await.unwrap(),
                json!(7)
            );
        });
        assert_eq!(domain.reads.load(Ordering::SeqCst), 1);
        assert_eq!(domain.commands.load(Ordering::SeqCst), 0);
        next.shutdown().unwrap();
    }

    #[test]
    fn admitted_read_may_finish_after_withdrawal_but_new_reads_cannot_dispatch() {
        use tokio::sync::oneshot;

        struct PendingRead {
            started: Mutex<Option<oneshot::Sender<()>>>,
            release: Mutex<Option<oneshot::Receiver<()>>>,
            invocations: AtomicUsize,
        }
        impl AsyncRpcHandler for PendingRead {
            fn contract(&self) -> Result<CapabilityContract, RpcError> {
                let contract = Domain::new().contract()?;
                Ok(contract)
            }
            fn dispatch<'a>(&'a self, operation: &'a str, _arguments: Value) -> AsyncRpcFuture<'a> {
                Box::pin(async move {
                    assert_eq!(operation, "count");
                    self.invocations.fetch_add(1, Ordering::SeqCst);
                    let started = self.started.lock().unwrap().take().unwrap();
                    let release = self.release.lock().unwrap().take().unwrap();
                    started.send(()).unwrap();
                    release.await.unwrap();
                    Ok(json!(7))
                })
            }
        }

        let (started_tx, started_rx) = oneshot::channel();
        let (release_tx, release_rx) = oneshot::channel();
        let domain = Arc::new(PendingRead {
            started: Mutex::new(Some(started_tx)),
            release: Mutex::new(Some(release_rx)),
            invocations: AtomicUsize::new(0),
        });
        let mut running = app();
        let publication = Arc::new(
            running
                .publish_local_reads(
                    "research.app",
                    "graph.visible",
                    ReadExposure::NonSensitiveSharedUser,
                    domain.clone(),
                    ["count".into()],
                )
                .unwrap(),
        );
        runtime().block_on(async {
            let descriptor = publication.descriptor();
            let active = Arc::clone(&publication);
            let request = descriptor.clone();
            let prior = tokio::spawn(async move {
                active.query(&request, "count", json!({})).await
            });
            started_rx.await.unwrap();
            assert_eq!(publication.runtime_ingress.in_flight(), 1);
            publication.withdraw();
            assert!(matches!(
                publication.query(&descriptor, "count", json!({})).await,
                Err(ReadPublicationError::Withdrawn)
            ));
            assert_eq!(domain.invocations.load(Ordering::SeqCst), 1);
            // Withdrawal does not roll back a read that acquired its
            // runtime admission permit before the withdrawal transition.
            release_tx.send(()).unwrap();
            assert_eq!(prior.await.unwrap(), Ok(json!(7)));
            assert_eq!(publication.runtime_ingress.in_flight(), 0);
        });
        running.shutdown().unwrap();
    }

    #[test]
    fn validation_rejects_empty_unsafe_or_unbounded_publication() {
        let mut running = app();
        let domain = Arc::new(Domain::new());
        for selected in [
            Vec::<String>::new(),
            vec!["unregistered".into()],
            (0..129).map(|n| format!("count{n}")).collect(),
        ] {
            assert!(
                running
                    .publish_local_reads(
                        "research.app",
                        "graph.visible",
                        ReadExposure::NonSensitiveSharedUser,
                        domain.clone(),
                        selected
                    )
                    .is_err()
            );
        }
        assert!(
            running
                .publish_local_reads(
                    "",
                    "graph.visible",
                    ReadExposure::NonSensitiveSharedUser,
                    domain.clone(),
                    ["count".into()]
                )
                .is_err()
        );
        assert_eq!(domain.reads.load(Ordering::SeqCst), 0);
        assert_eq!(domain.commands.load(Ordering::SeqCst), 0);
        running.shutdown().unwrap();
    }
}
