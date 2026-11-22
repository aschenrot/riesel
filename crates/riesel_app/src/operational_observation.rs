//! #158 internal trusted-local operational evidence, not an SDK public
//! health API, wire protocol, authorization decision or process supervisor.
//!
//! A snapshot says what **one owner** actually observed at one instant. It
//! cannot turn a verified provider contract or bound listener into an
//! authenticated/callable application operation. It neither polls nor retains
//! history, includes no free-form error payload, and does not run user code.
use crate::{
    InstanceIdentity, InternalEntrypointActivity, InternalEntrypointPhase, ResolvedCapability,
};
use riesel_manager::{OwnedAttemptEvidence, OwnedAttemptPhase, ProcessKey, ProcessRegistry};
use riesel_protocol::SessionToken;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Maximum number of records in a single on-demand, in-process snapshot.
pub const MAX_OPERATIONAL_FACTS: usize = 64;
/// Evidence ages out; an old success is not proof of continued availability.
pub const MAX_OBSERVATION_AGE: Duration = Duration::from_secs(5);
const MAX_OBSERVATION_LABEL_BYTES: usize = 128;

/// The owner of the *observation*, not a claimed remote principal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationSource {
    Runtime,
    ApplicationAdapter,
    /// The existing ProcessRegistry owns the actual local Child handle.
    /// Not an authenticated installed Manager OS attempt.
    ProcessOwner,
}

/// How the snapshot's runtime-generation reference was obtained. An
/// application adapter can reference a generation but cannot issue or
/// independently attest it as a Riesel-runtime identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GenerationEvidence {
    RuntimeIssued,
    AdapterReferenceOnly,
}

/// Typed, non-interchangeable scopes. The whole-runtime generation is carried
/// by the snapshot, not invented from a PID, address or provider ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObservationSubject {
    RuntimeSession,
    /// SDK Rust callback execution, not application interface readiness.
    ApplicationEntrypoint,
    ProviderBinding {
        requirement: String,
        capability: String,
        version: u32,
        provider: String,
        instance: Option<InstanceIdentity>,
    },
    LocalApplicationListener {
        interface: String,
        address: SocketAddr,
    },
    AuthorizedApplicationOperation,
    OsLaunchAttempt,
}

/// Evidence is deliberately narrow: none of these is a public client grant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationEvidence {
    RuntimeSessionValidated,
    SdkEntrypointNotEntered,
    SdkEntrypointExecuting,
    SdkEntrypointLeft,
    VersionedProviderContractAttached,
    LocalListenerBound,
    OwnedChildHandlePolled,
    SourceDoesNotVerify,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationState {
    /// The named evidence was directly witnessed at the snapshot time; it is
    /// *not* a permanent claim that the subject remains alive.
    Observed,
    /// Current evidence was missing or a verification attempt failed.
    Unknown,
    /// This source has no authority to answer that specific question.
    Unsupported,
    /// A previously witnessed fact is no longer current for this session or
    /// has aged beyond the fixed bound. This is *not* evidence of shutdown.
    Stale,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationFact {
    pub source: ObservationSource,
    pub subject: ObservationSubject,
    pub evidence: ObservationEvidence,
    pub state: ObservationState,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ObservationError {
    #[error("operational snapshot exceeds its fixed fact budget")]
    TooManyFacts,
    #[error("operational subject label is empty or exceeds its fixed limit")]
    InvalidLabel,
    #[error("local listener evidence requires a bound numeric loopback socket")]
    InvalidLoopbackListener,
    #[error("entrypoint witness does not belong to this runtime session")]
    ForeignEntrypointSession,
    #[error("operation identifier must be a bounded static-style name")]
    InvalidOperationIdentifier,
    #[error("cannot issue new operation evidence after the runtime session expired")]
    ExpiredOperationSession,
}

/// Immutable, on-demand evidence from one actual runtime generation.
/// Runtime SessionToken is held only in process; generation is a nonsecret
/// correlation string, not a credential, client verifier or OS attempt ID.
#[derive(Clone, Debug)]
pub struct ObservationSnapshot {
    generation: String,
    generation_evidence: GenerationEvidence,
    observed_at: Instant,
    session: SessionToken,
    facts: Vec<ObservationFact>,
    entrypoint_witness: Option<(usize, InternalEntrypointActivity, InternalEntrypointPhase)>,
}

impl ObservationSnapshot {
    fn new(session: SessionToken, generation_evidence: GenerationEvidence) -> Self {
        let generation = session.internal_publication_session().generation();
        Self {
            generation,
            generation_evidence,
            observed_at: Instant::now(),
            session,
            facts: Vec::new(),
            entrypoint_witness: None,
        }
    }

    fn push(&mut self, fact: ObservationFact) -> Result<(), ObservationError> {
        if self.facts.len() >= MAX_OPERATIONAL_FACTS {
            return Err(ObservationError::TooManyFacts);
        }
        self.facts.push(fact);
        Ok(())
    }

    fn check_label(label: &str) -> Result<(), ObservationError> {
        if label.is_empty() || label.len() > MAX_OBSERVATION_LABEL_BYTES {
            Err(ObservationError::InvalidLabel)
        } else {
            Ok(())
        }
    }

    fn unsupported(&mut self, source: ObservationSource) -> Result<(), ObservationError> {
        for subject in [
            ObservationSubject::AuthorizedApplicationOperation,
            ObservationSubject::OsLaunchAttempt,
        ] {
            self.push(ObservationFact {
                source,
                subject,
                evidence: ObservationEvidence::SourceDoesNotVerify,
                state: ObservationState::Unsupported,
            })?;
        }
        Ok(())
    }

    /// Called only by RunningApp after actual startup and the existing
    /// supervisor/task checks. "Verified binding" means versioned *internal*
    /// attachment, not live health, interface publication or callable grant.
    pub(crate) fn runtime(
        session: SessionToken,
        resolved: &[ResolvedCapability],
        checks_passed: bool,
    ) -> Result<Self, ObservationError> {
        Self::runtime_with_entrypoint(session, resolved, checks_passed, None)
    }

    pub(crate) fn runtime_with_entrypoint(
        session: SessionToken,
        resolved: &[ResolvedCapability],
        checks_passed: bool,
        entrypoint: Option<InternalEntrypointActivity>,
    ) -> Result<Self, ObservationError> {
        if resolved.len() > MAX_OPERATIONAL_FACTS - if entrypoint.is_some() { 4 } else { 3 } {
            return Err(ObservationError::TooManyFacts);
        }
        if entrypoint
            .as_ref()
            .is_some_and(|witness| !witness.same_session(&session))
        {
            return Err(ObservationError::ForeignEntrypointSession);
        }
        let mut snapshot = Self::new(session, GenerationEvidence::RuntimeIssued);
        let current = checks_passed && snapshot.session.is_active();
        snapshot.push(ObservationFact {
            source: ObservationSource::Runtime,
            subject: ObservationSubject::RuntimeSession,
            evidence: ObservationEvidence::RuntimeSessionValidated,
            state: if current {
                ObservationState::Observed
            } else {
                ObservationState::Unknown
            },
        })?;
        for binding in resolved {
            for label in [
                &binding.requirement,
                &binding.capability.id,
                &binding.provider,
            ] {
                Self::check_label(label)?;
            }
            match &binding.instance {
                Some(InstanceIdentity::Local { provider }) => Self::check_label(provider)?,
                Some(InstanceIdentity::Managed { name }) => Self::check_label(name)?,
                Some(InstanceIdentity::ExternalTcp { .. }) | None => {}
            }
            snapshot.push(ObservationFact {
                source: ObservationSource::Runtime,
                subject: ObservationSubject::ProviderBinding {
                    requirement: binding.requirement.clone(),
                    capability: binding.capability.id.clone(),
                    version: binding.capability.version,
                    provider: binding.provider.clone(),
                    instance: binding.instance.clone(),
                },
                evidence: ObservationEvidence::VersionedProviderContractAttached,
                state: if current && binding.instance.is_some() {
                    ObservationState::Observed
                } else {
                    ObservationState::Unknown
                },
            })?;
        }
        if let Some(entrypoint) = entrypoint {
            let phase = entrypoint.phase();
            let evidence = match phase {
                InternalEntrypointPhase::NotEntered => ObservationEvidence::SdkEntrypointNotEntered,
                InternalEntrypointPhase::Executing => ObservationEvidence::SdkEntrypointExecuting,
                InternalEntrypointPhase::Left => ObservationEvidence::SdkEntrypointLeft,
            };
            snapshot.entrypoint_witness = Some((snapshot.facts.len(), entrypoint, phase));
            snapshot.push(ObservationFact {
                source: ObservationSource::Runtime,
                subject: ObservationSubject::ApplicationEntrypoint,
                evidence,
                state: if current {
                    ObservationState::Observed
                } else {
                    ObservationState::Unknown
                },
            })?;
        }
        snapshot.unsupported(ObservationSource::Runtime)?;
        Ok(snapshot)
    }

    /// The *application adapter*, after successfully binding its own socket,
    /// may attest that historical event. The caller must supply an actual
    /// runtime session. This constructor NEVER verifies remote/client access.
    ///
    /// It is intentionally in the internal crate, not the public SDK facade.
    pub fn local_listener(
        session: SessionToken,
        interface: &str,
        address: SocketAddr,
    ) -> Result<Self, ObservationError> {
        Self::check_label(interface)?;
        if !address.ip().is_loopback() || address.port() == 0 {
            return Err(ObservationError::InvalidLoopbackListener);
        }
        let mut snapshot = Self::new(session, GenerationEvidence::AdapterReferenceOnly);
        let active = snapshot.session.is_active();
        snapshot.push(ObservationFact {
            source: ObservationSource::ApplicationAdapter,
            subject: ObservationSubject::LocalApplicationListener {
                interface: interface.to_owned(),
                address,
            },
            evidence: ObservationEvidence::LocalListenerBound,
            state: if active {
                ObservationState::Observed
            } else {
                ObservationState::Unknown
            },
        })?;
        snapshot.unsupported(ObservationSource::ApplicationAdapter)?;
        Ok(snapshot)
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn generation_evidence(&self) -> GenerationEvidence {
        self.generation_evidence
    }

    pub fn observed_at(&self) -> Instant {
        self.observed_at
    }

    pub fn facts(&self) -> &[ObservationFact] {
        &self.facts
    }

    /// Read the fact through a *bounded* freshness lens without mutating it.
    /// A closed runtime or elapsed observation becomes stale, not "stopped".
    /// Unsupported and unknown remain explicit even after time passes.
    pub fn state_at(&self, index: usize, at: Instant) -> Option<ObservationState> {
        let state = self.facts.get(index)?.state;
        if state != ObservationState::Observed {
            return Some(state);
        }
        if !self.session.is_active()
            || self
                .entrypoint_witness
                .as_ref()
                .is_some_and(|(fact_index, owner, phase)| {
                    *fact_index == index && owner.phase() != *phase
                })
            || at.saturating_duration_since(self.observed_at) > MAX_OBSERVATION_AGE
        {
            Some(ObservationState::Stale)
        } else {
            Some(ObservationState::Observed)
        }
    }

    /// No missing fact can be interpreted as an observed success.
    pub fn state_of(&self, subject: &ObservationSubject, at: Instant) -> ObservationState {
        self.facts
            .iter()
            .position(|fact| &fact.subject == subject)
            .and_then(|index| self.state_at(index, at))
            .unwrap_or(ObservationState::Unknown)
    }
}

/// An application adapter's **reported** dispatch/effect boundary. These
/// names have source-specific meanings: an adapter can report an event in
/// its own handler, but cannot attest a remote client's identity/permission
/// or make a durable Manager/application effect receipt out of this value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterOperationEvidence {
    /// Admission/validation definitively refused before domain dispatch by
    /// *this adapter*. Never use for a transport timeout or uncertain peer.
    RejectedBeforeDomainDispatch,
    /// The owning domain returned success. The response to an external peer
    /// may nevertheless be lost later on its independent transport.
    DomainConfirmedSuccess,
    /// The owning domain returned a typed failure. A failure does not imply
    /// no effects unless the domain separately guarantees atomicity.
    DomainConfirmedFailure,
    /// A reply was lost after work was admitted. Dispatch may have occurred,
    /// including a persisted mutation; retrying is not justified.
    AdmittedReplyLost,
    /// An app-owned multi-stage operation can prove at least one effect but
    /// cannot claim the entire intended operation succeeded. Reconcile first.
    ConfirmedPartialRequiresReconciliation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationDisposition {
    RejectedBeforeDispatch,
    ConfirmedSuccess,
    ConfirmedDomainFailure,
    OutcomeUnknown,
    PartialNeedsReconciliation,
}

impl AdapterOperationEvidence {
    pub fn disposition(self) -> OperationDisposition {
        match self {
            Self::RejectedBeforeDomainDispatch => OperationDisposition::RejectedBeforeDispatch,
            Self::DomainConfirmedSuccess => OperationDisposition::ConfirmedSuccess,
            Self::DomainConfirmedFailure => OperationDisposition::ConfirmedDomainFailure,
            Self::AdmittedReplyLost => OperationDisposition::OutcomeUnknown,
            Self::ConfirmedPartialRequiresReconciliation => {
                OperationDisposition::PartialNeedsReconciliation
            }
        }
    }
}

/// Exactly one on-demand, in-process *adapter-reported* event. This is not a
/// queryable global operation log; no receipt IDs, arguments, errors, client
/// identifiers or mutable ring buffers are retained. The observer must supply
/// an actual session reference, which is not independent runtime attestation.
#[derive(Clone, Debug)]
pub struct AdapterOperationSnapshot {
    operation: String,
    event: AdapterOperationEvidence,
    observed_at: Instant,
    session: SessionToken,
    generation: String,
}

impl AdapterOperationSnapshot {
    pub fn report(
        session: SessionToken,
        operation: &str,
        event: AdapterOperationEvidence,
    ) -> Result<Self, ObservationError> {
        if operation.is_empty()
            || operation.len() > MAX_OBSERVATION_LABEL_BYTES
            || !operation
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(ObservationError::InvalidOperationIdentifier);
        }
        if !session.is_active() {
            return Err(ObservationError::ExpiredOperationSession);
        }
        Ok(Self {
            operation: operation.to_owned(),
            event,
            observed_at: Instant::now(),
            generation: session.internal_publication_session().generation(),
            session,
        })
    }

    pub fn source(&self) -> ObservationSource {
        ObservationSource::ApplicationAdapter
    }

    pub fn generation_evidence(&self) -> GenerationEvidence {
        GenerationEvidence::AdapterReferenceOnly
    }

    pub fn generation(&self) -> &str {
        &self.generation
    }

    pub fn operation(&self) -> &str {
        &self.operation
    }

    pub fn event(&self) -> AdapterOperationEvidence {
        self.event
    }

    pub fn disposition(&self) -> OperationDisposition {
        self.event.disposition()
    }

    pub fn observed_at(&self) -> Instant {
        self.observed_at
    }

    /// A retained event may be shown as historical evidence, but must not
    /// be promoted to the current application's outcome after session death.
    pub fn state_at(&self, at: Instant) -> ObservationState {
        if !self.session.is_active()
            || at < self.observed_at
            || at.duration_since(self.observed_at) > MAX_OBSERVATION_AGE
        {
            ObservationState::Stale
        } else {
            ObservationState::Observed
        }
    }
}

/// Independently scoped source observation of the *actual owned OS Child*.
/// Unlike a Riesel application snapshot it has no application generation:
/// the process owner is a different authority and the two snapshots cannot
/// be atomically joined or promoted to a shared global identity.
///
/// A ProcessKey is meaningful only within this ProcessRegistry's ownership
/// lifetime. The privately held weak lease distinguishes repeated use of the
/// same key without introducing a production Manager registration identity.
#[derive(Clone, Debug)]
pub struct ProcessOwnerSnapshot {
    key: ProcessKey,
    observed_at: Instant,
    fact: ObservationFact,
    owned: Option<OwnedAttemptEvidence>,
}

impl ProcessOwnerSnapshot {
    /// Poll once via the *existing* registry's retained Child; never create
    /// another process owner or bypass the registry. Missing/inaccessible
    /// evidence becomes Unknown, not "exited", "stopped", or authenticated.
    pub fn sample(
        registry: &mut ProcessRegistry,
        key: &ProcessKey,
    ) -> Result<Self, ObservationError> {
        if let Some(name) = &key.name {
            ObservationSnapshot::check_label(name)?;
        }
        let owned = registry.observe_owned_attempt(key).ok();
        let state = if owned.is_some() {
            ObservationState::Observed
        } else {
            ObservationState::Unknown
        };
        Ok(Self {
            key: key.clone(),
            observed_at: Instant::now(),
            fact: ObservationFact {
                source: ObservationSource::ProcessOwner,
                subject: ObservationSubject::OsLaunchAttempt,
                evidence: if owned.is_some() {
                    ObservationEvidence::OwnedChildHandlePolled
                } else {
                    ObservationEvidence::SourceDoesNotVerify
                },
                state,
            },
            owned,
        })
    }

    pub fn registry_key(&self) -> &ProcessKey {
        &self.key
    }

    pub fn observed_at(&self) -> Instant {
        self.observed_at
    }

    pub fn fact(&self) -> &ObservationFact {
        &self.fact
    }

    /// Running/Exited are statuses of one actual OS child **at the sampled
    /// instant**, not the health of an application, its provider, or a grant.
    pub fn observed_phase(&self) -> Option<OwnedAttemptPhase> {
        self.owned.as_ref().map(OwnedAttemptEvidence::phase)
    }

    pub fn same_owned_attempt_as(&self, other: &Self) -> bool {
        match (&self.owned, &other.owned) {
            (Some(a), Some(b)) => a.same_owned_attempt_as(b),
            _ => false,
        }
    }

    pub fn state_at(&self, at: Instant) -> ObservationState {
        match &self.owned {
            Some(observed) if observed.is_current_at(at, MAX_OBSERVATION_AGE) => {
                ObservationState::Observed
            }
            Some(_) => ObservationState::Stale,
            None => ObservationState::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstanceIdentity;
    use riesel_schema::CapabilityRef;
    use std::net::TcpListener;

    #[test]
    fn process_owner_probe_child() {
        if std::env::var_os("RIESEL_OPERATIONAL_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(15));
        }
    }

    fn owned_child() -> std::process::Command {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "operational_observation::tests::process_owner_probe_child",
            ])
            .env("RIESEL_OPERATIONAL_CHILD", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        command
    }

    #[test]
    fn process_owned_attempt_has_no_riesel_runtime_or_interface_authority() {
        let key = ProcessKey::new(82, Some("counter-foreground".into()));
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut owned_child()).unwrap();
        let first = ProcessOwnerSnapshot::sample(&mut registry, &key).unwrap();
        assert_eq!(first.observed_phase(), Some(OwnedAttemptPhase::Running));
        assert_eq!(first.fact().source, ObservationSource::ProcessOwner);
        assert_eq!(first.fact().subject, ObservationSubject::OsLaunchAttempt);
        assert_eq!(
            first.fact().evidence,
            ObservationEvidence::OwnedChildHandlePolled
        );
        assert_eq!(first.state_at(Instant::now()), ObservationState::Observed);

        // A missing registry child is *unknown* rather than stopped.
        let missing = ProcessOwnerSnapshot::sample(
            &mut registry,
            &ProcessKey::new(83, Some("unowned".into())),
        )
        .unwrap();
        assert_eq!(missing.state_at(Instant::now()), ObservationState::Unknown);
        assert_eq!(missing.observed_phase(), None);

        // Riesel's runtime does not acquire process-owner authority just
        // because this same Rust program contains both observers.
        let session = SessionToken::new();
        let runtime = ObservationSnapshot::runtime(session.clone(), &[], true).unwrap();
        assert_eq!(
            runtime.state_of(&ObservationSubject::OsLaunchAttempt, Instant::now()),
            ObservationState::Unsupported
        );
        assert_eq!(
            runtime.state_of(
                &ObservationSubject::AuthorizedApplicationOperation,
                Instant::now()
            ),
            ObservationState::Unsupported
        );

        registry.stop(&key).unwrap();
        assert_eq!(first.state_at(Instant::now()), ObservationState::Stale);
        let no_longer_owned = ProcessOwnerSnapshot::sample(&mut registry, &key).unwrap();
        assert_eq!(
            no_longer_owned.state_at(Instant::now()),
            ObservationState::Unknown
        );

        // Reuse of a human-readable ProcessKey in the same registry is not
        // the previous OS attempt, and cannot refresh an old observation.
        registry.spawn(key.clone(), &mut owned_child()).unwrap();
        let second = ProcessOwnerSnapshot::sample(&mut registry, &key).unwrap();
        assert!(!first.same_owned_attempt_as(&second));
        assert_eq!(second.registry_key(), &key);
        assert_eq!(second.observed_phase(), Some(OwnedAttemptPhase::Running));
        assert_eq!(second.state_at(Instant::now()), ObservationState::Observed);
        assert_eq!(first.state_at(Instant::now()), ObservationState::Stale);
        assert_eq!(
            second.state_at(second.observed_at() + MAX_OBSERVATION_AGE + Duration::from_nanos(1)),
            ObservationState::Stale
        );
        registry.stop_all().unwrap();
        assert_eq!(second.state_at(Instant::now()), ObservationState::Stale);
        session.invalidate();
    }

    #[test]
    fn runtime_entrypoint_phase_is_owned_and_stales_on_lexical_transitions() {
        use crate::{AsyncTasks, CancellationToken};
        let session = SessionToken::new();
        let tasks = AsyncTasks::new(session.clone(), CancellationToken::default());
        let activity = tasks.internal_entrypoint_activity();

        let before = ObservationSnapshot::runtime_with_entrypoint(
            session.clone(),
            &[],
            true,
            Some(activity.clone()),
        )
        .unwrap();
        let subject = ObservationSubject::ApplicationEntrypoint;
        assert_eq!(before.facts().len(), 4);
        assert_eq!(
            before.facts()[1].evidence,
            ObservationEvidence::SdkEntrypointNotEntered
        );
        assert_eq!(
            before.state_of(&subject, Instant::now()),
            ObservationState::Observed
        );
        assert_eq!(
            before.state_of(
                &ObservationSubject::AuthorizedApplicationOperation,
                Instant::now()
            ),
            ObservationState::Unsupported
        );

        let active_guard = tasks.internal_enter_entrypoint().unwrap();
        assert_eq!(activity.phase(), InternalEntrypointPhase::Executing);
        assert_eq!(
            before.state_of(&subject, Instant::now()),
            ObservationState::Stale
        );
        assert!(tasks.internal_enter_entrypoint().is_err());

        let executing = ObservationSnapshot::runtime_with_entrypoint(
            session.clone(),
            &[],
            true,
            Some(activity.clone()),
        )
        .unwrap();
        assert_eq!(
            executing.facts()[1].evidence,
            ObservationEvidence::SdkEntrypointExecuting
        );
        assert_eq!(
            executing.state_of(&subject, Instant::now()),
            ObservationState::Observed
        );

        drop(active_guard);
        assert_eq!(activity.phase(), InternalEntrypointPhase::Left);
        assert_eq!(
            executing.state_of(&subject, Instant::now()),
            ObservationState::Stale
        );
        let left = ObservationSnapshot::runtime_with_entrypoint(
            session.clone(),
            &[],
            true,
            Some(activity.clone()),
        )
        .unwrap();
        assert_eq!(
            left.facts()[1].evidence,
            ObservationEvidence::SdkEntrypointLeft
        );
        assert_eq!(
            left.state_of(&subject, Instant::now()),
            ObservationState::Observed
        );

        let unrelated = SessionToken::new();
        assert!(matches!(
            ObservationSnapshot::runtime_with_entrypoint(unrelated, &[], true, Some(activity)),
            Err(ObservationError::ForeignEntrypointSession)
        ));
        session.invalidate();
        assert_eq!(
            left.state_of(&subject, Instant::now()),
            ObservationState::Stale
        );
        assert!(tasks.internal_enter_entrypoint().is_err());
    }

    #[test]
    fn adapter_outcome_event_mapping_is_finite_source_qualified_and_session_fenced() {
        let session = SessionToken::new();
        let expected = [
            (
                AdapterOperationEvidence::RejectedBeforeDomainDispatch,
                OperationDisposition::RejectedBeforeDispatch,
            ),
            (
                AdapterOperationEvidence::DomainConfirmedSuccess,
                OperationDisposition::ConfirmedSuccess,
            ),
            (
                AdapterOperationEvidence::DomainConfirmedFailure,
                OperationDisposition::ConfirmedDomainFailure,
            ),
            (
                AdapterOperationEvidence::AdmittedReplyLost,
                OperationDisposition::OutcomeUnknown,
            ),
            (
                AdapterOperationEvidence::ConfirmedPartialRequiresReconciliation,
                OperationDisposition::PartialNeedsReconciliation,
            ),
        ];
        for (event, disposition) in expected {
            let sample =
                AdapterOperationSnapshot::report(session.clone(), "todo.task.create", event)
                    .unwrap();
            assert_eq!(sample.source(), ObservationSource::ApplicationAdapter);
            assert_eq!(
                sample.generation_evidence(),
                GenerationEvidence::AdapterReferenceOnly
            );
            assert_eq!(
                sample.generation(),
                session.internal_publication_session().generation()
            );
            assert_eq!(sample.operation(), "todo.task.create");
            assert_eq!(sample.event(), event);
            assert_eq!(sample.disposition(), disposition);
            assert_eq!(sample.state_at(Instant::now()), ObservationState::Observed);
            assert_eq!(
                sample
                    .state_at(sample.observed_at() + MAX_OBSERVATION_AGE + Duration::from_nanos(1)),
                ObservationState::Stale
            );
        }
        for invalid in ["", "task / secret", "task\nsecret", &"x".repeat(129)] {
            assert!(matches!(
                AdapterOperationSnapshot::report(
                    session.clone(),
                    invalid,
                    AdapterOperationEvidence::DomainConfirmedSuccess
                ),
                Err(ObservationError::InvalidOperationIdentifier)
            ));
        }
        let old = AdapterOperationSnapshot::report(
            session.clone(),
            "counter.add",
            AdapterOperationEvidence::AdmittedReplyLost,
        )
        .unwrap();
        session.invalidate();
        assert_eq!(old.state_at(Instant::now()), ObservationState::Stale);
        assert!(matches!(
            AdapterOperationSnapshot::report(
                session,
                "counter.add",
                AdapterOperationEvidence::DomainConfirmedSuccess
            ),
            Err(ObservationError::ExpiredOperationSession)
        ));
    }

    #[test]
    fn verified_binding_does_not_authorize_an_interface_or_os_attempt() {
        let session = SessionToken::new();
        let resolved = vec![ResolvedCapability {
            requirement: "counter".into(),
            capability: CapabilityRef {
                id: "example.counter".into(),
                version: 1,
            },
            provider: "example.counter.provider".into(),
            instance: Some(InstanceIdentity::Managed {
                name: "counter-managed".into(),
            }),
        }];
        let frame = ObservationSnapshot::runtime(session.clone(), &resolved, true).unwrap();
        assert_eq!(
            frame.generation(),
            session.internal_publication_session().generation()
        );
        assert_eq!(
            frame.generation_evidence(),
            GenerationEvidence::RuntimeIssued
        );
        assert_eq!(frame.facts().len(), 4);
        assert_eq!(
            frame.state_at(1, frame.observed_at()),
            Some(ObservationState::Observed)
        );
        assert_eq!(
            frame.state_of(
                &ObservationSubject::AuthorizedApplicationOperation,
                Instant::now()
            ),
            ObservationState::Unsupported
        );
        assert_eq!(
            frame.state_of(&ObservationSubject::OsLaunchAttempt, Instant::now()),
            ObservationState::Unsupported
        );
        assert_eq!(
            frame.state_of(
                &ObservationSubject::LocalApplicationListener {
                    interface: "counter".into(),
                    address: "127.0.0.1:9999".parse().unwrap(),
                },
                Instant::now()
            ),
            ObservationState::Unknown
        );
    }

    #[test]
    fn generation_revocation_and_age_make_old_success_stale_not_failed() {
        let a = SessionToken::new();
        let old = ObservationSnapshot::runtime(a.clone(), &[], true).unwrap();
        let b = SessionToken::new();
        let next = ObservationSnapshot::runtime(b.clone(), &[], true).unwrap();
        assert_ne!(old.generation(), next.generation());
        a.invalidate();
        assert_eq!(
            old.state_at(0, Instant::now()),
            Some(ObservationState::Stale)
        );
        assert_eq!(
            next.state_at(0, Instant::now()),
            Some(ObservationState::Observed)
        );
        assert_eq!(
            next.state_at(
                0,
                next.observed_at() + MAX_OBSERVATION_AGE + Duration::from_nanos(1)
            ),
            Some(ObservationState::Stale)
        );
        assert_eq!(
            next.state_at(2, Instant::now()),
            Some(ObservationState::Unsupported)
        );
    }

    #[test]
    fn real_bound_listener_is_not_proof_of_authorized_operation() {
        let socket = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        let session = SessionToken::new();
        let frame =
            ObservationSnapshot::local_listener(session.clone(), "todo.http", address).unwrap();
        assert_eq!(
            frame.facts()[0].source,
            ObservationSource::ApplicationAdapter
        );
        assert_eq!(
            frame.generation_evidence(),
            GenerationEvidence::AdapterReferenceOnly
        );
        assert_eq!(
            frame.state_at(0, Instant::now()),
            Some(ObservationState::Observed)
        );
        assert_eq!(
            frame.state_of(
                &ObservationSubject::AuthorizedApplicationOperation,
                Instant::now()
            ),
            ObservationState::Unsupported
        );
        drop(socket);
        // After closing the socket, a *snapshot* of a past bind does not
        // become evidence of an active listener; freshness is bounded.
        assert_eq!(
            frame.state_at(
                0,
                frame.observed_at() + MAX_OBSERVATION_AGE + Duration::from_secs(1)
            ),
            Some(ObservationState::Stale)
        );
        session.invalidate();
        assert_eq!(
            frame.state_at(0, Instant::now()),
            Some(ObservationState::Stale)
        );
    }

    #[test]
    fn label_and_memory_limits_fail_explicitly_without_truncating_evidence() {
        let session = SessionToken::new();
        let bound = "127.0.0.1:8080".parse().unwrap();
        assert!(matches!(
            ObservationSnapshot::local_listener(session.clone(), "", bound),
            Err(ObservationError::InvalidLabel)
        ));
        assert!(matches!(
            ObservationSnapshot::local_listener(session.clone(), &"x".repeat(129), bound),
            Err(ObservationError::InvalidLabel)
        ));
        assert!(matches!(
            ObservationSnapshot::local_listener(
                session.clone(),
                "bad",
                "0.0.0.0:8080".parse().unwrap()
            ),
            Err(ObservationError::InvalidLoopbackListener)
        ));
        assert!(matches!(
            ObservationSnapshot::runtime(
                session,
                &vec![
                    ResolvedCapability {
                        requirement: "c".into(),
                        capability: CapabilityRef {
                            id: "example.c".into(),
                            version: 1
                        },
                        provider: "p".into(),
                        instance: None,
                    };
                    MAX_OPERATIONAL_FACTS - 2
                ],
                true,
            ),
            Err(ObservationError::TooManyFacts)
        ));
    }

    #[test]
    fn unknown_failed_verification_is_not_a_definite_provider_failure() {
        let session = SessionToken::new();
        let bound = ResolvedCapability {
            requirement: "counter".into(),
            capability: CapabilityRef {
                id: "example.counter".into(),
                version: 1,
            },
            provider: "test".into(),
            instance: None,
        };
        let snapshot = ObservationSnapshot::runtime(session, &[bound], false).unwrap();
        assert_eq!(
            snapshot.state_at(0, Instant::now()),
            Some(ObservationState::Unknown)
        );
        assert_eq!(
            snapshot.state_at(1, Instant::now()),
            Some(ObservationState::Unknown)
        );
        assert_eq!(
            snapshot.state_at(2, Instant::now()),
            Some(ObservationState::Unsupported)
        );
    }
}
