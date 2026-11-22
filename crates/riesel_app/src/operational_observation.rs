//! #158 internal trusted-local operational evidence, not an SDK public
//! health API, wire protocol, authorization decision or process supervisor.
//!
//! A snapshot says what **one owner** actually observed at one instant. It
//! cannot turn a verified provider contract or bound listener into an
//! authenticated/callable application operation. It neither polls nor retains
//! history, includes no free-form error payload, and does not run user code.
use crate::{InstanceIdentity, ResolvedCapability};
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
    VersionedProviderContractAttached,
    LocalListenerBound,
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
        if resolved.len() > MAX_OPERATIONAL_FACTS - 3 {
            return Err(ObservationError::TooManyFacts);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InstanceIdentity;
    use riesel_schema::CapabilityRef;
    use std::net::TcpListener;

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
