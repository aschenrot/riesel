//! RESEARCH ONLY: provisional application-owned export policy over existing
//! generated capability dispatch. This is not a wire protocol, credential
//! verifier, listener or stable SDK API. A trusted adapter MUST supply identity.
use riesel_protocol::{AsyncRpcHandler, RpcError};
use riesel_schema::{CapabilityContract, CapabilityRef, OperationKind};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use thiserror::Error;

/// The caller identity must come from an independently verified local or
/// protected transport. This constructor is intentionally a simulation
/// shortcut: do not expose it on a production trust boundary.
#[derive(Debug, Clone)]
pub struct SimulatedPrincipal(String);

impl SimulatedPrincipal {
    pub fn for_test(name: &str) -> Self {
        Self(name.to_owned())
    }
}

#[derive(Debug, Clone)]
pub struct AttachmentRequest {
    pub application: String,
    pub interface: String,
    pub generation: String,
    pub contract: CapabilityRef,
}

/// A local test ticket, not a bearer credential. Future wire requests must
/// be authenticated and generation-checked by the *server* on every call.
#[derive(Debug, Clone)]
pub struct Attachment {
    application: String,
    interface: String,
    generation: String,
    contract: CapabilityRef,
    principal: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExportError {
    #[error("export declaration/policy invalid: {0}")]
    Invalid(String),
    #[error("application or interface identity mismatch")]
    IdentityMismatch,
    #[error("export session generation is stale")]
    StaleGeneration,
    #[error("capability contract or version mismatch")]
    ContractMismatch,
    #[error("export is not accepting calls")]
    NotReady,
    #[error("caller has no grant")]
    Unauthorized,
    #[error("operation not allowed for caller")]
    Forbidden,
    #[error("unknown operation")]
    UnknownOperation,
    #[error("mutation outcome unknown; reconcile from authoritative state")]
    UnknownMutationOutcome,
    #[error(transparent)]
    Rpc(#[from] RpcError),
}

/// This wrapper owns *policy*, not the application domain or provider registry.
/// A new instance is required per running application session. The public
/// interface uses exactly one SDK-generated contract; no schema is duplicated.
pub struct ResearchExport {
    application: String,
    interface: String,
    generation: String,
    contract: CapabilityContract,
    grants: BTreeMap<String, BTreeSet<String>>,
    handler: Arc<dyn AsyncRpcHandler>,
    accepting: AtomicBool,
}

impl ResearchExport {
    pub fn new(
        application: &str,
        interface: &str,
        generation: &str,
        handler: Arc<dyn AsyncRpcHandler>,
        grants: BTreeMap<String, BTreeSet<String>>,
    ) -> Result<Self, ExportError> {
        if application.trim().is_empty()
            || interface.trim().is_empty()
            || generation.trim().is_empty()
        {
            return Err(ExportError::Invalid("empty identity".into()));
        }
        handler.validate_host()?;
        let contract = handler.contract()?;
        if grants.is_empty() {
            return Err(ExportError::Invalid("no authorized principals".into()));
        }
        for (principal, operations) in &grants {
            if principal.trim().is_empty() || operations.is_empty() {
                return Err(ExportError::Invalid(
                    "empty principal or operation grant".into(),
                ));
            }
            for operation in operations {
                if !contract.operations.contains_key(operation) {
                    return Err(ExportError::Invalid(format!(
                        "operation {operation} is not declared by the selected contract"
                    )));
                }
            }
        }
        Ok(Self {
            application: application.to_owned(),
            interface: interface.to_owned(),
            generation: generation.to_owned(),
            contract,
            grants,
            handler,
            accepting: AtomicBool::new(true),
        })
    }

    pub fn descriptor(&self) -> AttachmentRequest {
        AttachmentRequest {
            application: self.application.clone(),
            interface: self.interface.clone(),
            generation: self.generation.clone(),
            contract: self.contract.capability.clone(),
        }
    }

    pub fn attach(
        &self,
        request: &AttachmentRequest,
        trusted_identity: SimulatedPrincipal,
    ) -> Result<Attachment, ExportError> {
        self.check_descriptor(request)?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err(ExportError::NotReady);
        }
        if !self.grants.contains_key(&trusted_identity.0) {
            return Err(ExportError::Unauthorized);
        }
        Ok(Attachment {
            application: request.application.clone(),
            interface: request.interface.clone(),
            generation: request.generation.clone(),
            contract: request.contract.clone(),
            principal: trusted_identity.0,
        })
    }

    fn check_descriptor(&self, request: &AttachmentRequest) -> Result<(), ExportError> {
        if request.application != self.application || request.interface != self.interface {
            return Err(ExportError::IdentityMismatch);
        }
        if request.generation != self.generation {
            return Err(ExportError::StaleGeneration);
        }
        if request.contract != self.contract.capability {
            return Err(ExportError::ContractMismatch);
        }
        Ok(())
    }

    /// The caller must have a *fresh server-validated* attachment. The
    /// simulation has no cryptographic or OS-local proof of caller identity.
    pub async fn invoke(
        &self,
        attachment: &Attachment,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, ExportError> {
        self.check_descriptor(&AttachmentRequest {
            application: attachment.application.clone(),
            interface: attachment.interface.clone(),
            generation: attachment.generation.clone(),
            contract: attachment.contract.clone(),
        })?;
        if !self.accepting.load(Ordering::Acquire) {
            return Err(ExportError::NotReady);
        }
        let allowed = self
            .grants
            .get(&attachment.principal)
            .ok_or(ExportError::Unauthorized)?;
        let spec = self
            .contract
            .operations
            .get(operation)
            .ok_or(ExportError::UnknownOperation)?;
        if !allowed.contains(operation) {
            return Err(ExportError::Forbidden);
        }

        // The handler remains tied to the SDK-owned application session.
        // A transport adapter must enforce attachment checks before dispatch.
        let outcome = self
            .handler
            .dispatch_for(&self.contract.capability, operation, arguments)
            .await;
        if !self.accepting.load(Ordering::Acquire) {
            return if spec.kind == OperationKind::Command {
                Err(ExportError::UnknownMutationOutcome)
            } else {
                Err(ExportError::NotReady)
            };
        }
        match outcome {
            Err(RpcError::Transport(_) | RpcError::SessionExpired)
                if spec.kind == OperationKind::Command =>
            {
                Err(ExportError::UnknownMutationOutcome)
            }
            Ok(value) => Ok(value),
            Err(error) => Err(ExportError::Rpc(error)),
        }
    }

    /// Called by the owning application session on teardown. This closes
    /// admission; it does not cancel or roll back already-running commands.
    pub fn revoke(&self) {
        self.accepting.store(false, Ordering::Release);
    }
}

/// One test-only policy constructor shared by Counter and Todo fixtures.
pub fn grant(principal: &str, operations: &[&str]) -> BTreeMap<String, BTreeSet<String>> {
    BTreeMap::from([(
        principal.to_owned(),
        operations.iter().map(|item| (*item).to_owned()).collect(),
    )])
}
