//! Verify the SDK automatically reconstructs a complete session after a
//! managed provider dies. Old capability handles are never reused.
use riesel::Application;
use riesel_counter::CounterContract;

#[cfg(feature = "internal-publication")]
use riesel::internal_observation::{
    GenerationEvidence, ObservationSnapshot, ObservationSource, ObservationState,
    ObservationSubject,
};
use std::io;
use std::time::Duration;

#[cfg(feature = "internal-publication")]
use riesel_protocol::session::{InternalAdmissionError, InternalPublicationSession};

/// Deliberately participating local test adapter. The ingress guard protects
/// runtime generation and admission ordering only. No verified external
/// principal, public interface, transport or grant is provided here.
#[cfg(feature = "internal-publication")]
fn test_adapter_add(
    publication: &InternalPublicationSession,
    expected_generation: &str,
    counter: &std::sync::Arc<dyn riesel_counter::Counter + Send + Sync>,
    value: u64,
) -> riesel::AppResult<u64> {
    let _admitted = publication
        .try_admit(expected_generation)
        .map_err(|error| io::Error::other(format!("test ingress refused: {error:?}")))?;
    Ok(counter.add(value)?)
}

#[cfg(feature = "internal-publication")]
fn test_adapter_value(
    publication: &InternalPublicationSession,
    expected_generation: &str,
    counter: &std::sync::Arc<dyn riesel_counter::Counter + Send + Sync>,
) -> riesel::AppResult<u64> {
    let _admitted = publication
        .try_admit(expected_generation)
        .map_err(|error| io::Error::other(format!("test read refused: {error:?}")))?;
    Ok(counter.value()?)
}

fn main() -> riesel::Result<()> {
    let mut executions = 0;
    #[cfg(feature = "internal-publication")]
    let mut previous_publication: Option<(InternalPublicationSession, String, u32)> = None;
    let mut previous: Option<std::sync::Arc<dyn riesel_counter::Counter + Send + Sync>> = None;
    #[cfg(feature = "internal-publication")]
    let mut previous_observation: Option<ObservationSnapshot> = None;
    Application::new("RecoveryProbe")
        .rpc::<CounterContract>("counter")?
        .run_from_env(move |ctx| {
            executions += 1;
            let counter = ctx.capability::<CounterContract>("counter")?;
            assert_eq!(counter.value()?, 0);
            #[cfg(feature = "internal-publication")]
            {
                let observed = ctx
                    .internal_operational_observations()
                    .expect("runtime can observe only its own validated bindings");
                assert_eq!(
                    observed.generation(),
                    ctx.internal_publication_session().generation()
                );
                let binding_index = observed
                    .facts()
                    .iter()
                    .position(|fact| {
                        fact.source == ObservationSource::Runtime
                            && matches!(
                                &fact.subject,
                                ObservationSubject::ProviderBinding { requirement, .. }
                                    if requirement == "counter"
                            )
                    })
                    .expect("actual managed Counter binding must be observed");
                assert_eq!(
                    observed.state_at(binding_index, std::time::Instant::now()),
                    Some(ObservationState::Observed)
                );
                assert_eq!(
                    observed.state_of(
                        &ObservationSubject::AuthorizedApplicationOperation,
                        std::time::Instant::now(),
                    ),
                    ObservationState::Unsupported,
                    "verified managed Counter is not an exported authorized interface"
                );
                if executions == 1 {
                    previous_observation = Some(observed);
                } else {
                    let old = previous_observation
                        .take()
                        .expect("the first live runtime observation must exist");
                    assert_ne!(old.generation(), observed.generation());
                    assert_eq!(
                        old.state_at(0, std::time::Instant::now()),
                        Some(ObservationState::Stale),
                        "a real crash/recovery invalidates former runtime observations"
                    );
                    println!(
                        "SOURCE_QUALIFIED_COUNTER_OBSERVATION_OK                          verified_binding=true external_callability_unsupported=true                          old_generation_stale=true"
                    );
                }
            }
            if executions == 1 {
                #[cfg(feature = "internal-publication")]
                {
                    let publication = ctx.internal_publication_session();
                    let generation = publication.generation();
                    let pid = std::process::id();
                    assert_eq!(
                        test_adapter_value(&publication, &generation, counter)?,
                        0
                    );
                    assert_eq!(
                        test_adapter_add(&publication, &generation, counter, 2)?,
                        2
                    );
                    assert_eq!(counter.value()?, 2);
                    previous_publication = Some((publication, generation, pid));
                }
                previous = Some(counter.clone());
                if !ctx
                    .cancellation_token()
                    .wait_cancelled_timeout(Duration::from_secs(10))
                {
                    return Err(io::Error::other("crashing provider did not cancel session").into());
                }
            } else if executions == 2 {
                let old = previous.take().expect("old session handle exists");
                assert!(matches!(
                    old.value(),
                    Err(riesel_counter::CounterError::Transport(message))
                        if message.contains("expired application session")
                ));
                #[cfg(feature = "internal-publication")]
                {
                    let (old_publication, old_generation, original_pid) = previous_publication
                        .take()
                        .expect("first actual Riesel session was published");
                    let next_publication = ctx.internal_publication_session();
                    let next_generation = next_publication.generation();
                    assert_eq!(
                        std::process::id(),
                        original_pid,
                        "a whole-session retry must happen inside the same OS process"
                    );
                    assert_ne!(
                        old_generation, next_generation,
                        "the actual Riesel runtime must mint a fresh generation"
                    );
                    assert_eq!(
                        old_publication.try_admit(&old_generation).unwrap_err(),
                        InternalAdmissionError::Expired
                    );
                    assert_eq!(
                        next_publication.try_admit(&old_generation).unwrap_err(),
                        InternalAdmissionError::WrongGeneration
                    );
                    // Even if the new session has the same program/app name,
                    // the old adapter must reject before calling this new
                    // SDK-resolved Counter provider. No stale mutation occurs.
                    assert!(test_adapter_add(
                        &old_publication,
                        &old_generation,
                        counter,
                        99
                    ).is_err());
                    assert_eq!(counter.value()?, 0);
                    assert_eq!(
                        test_adapter_value(&next_publication, &next_generation, counter)?,
                        0
                    );
                    assert_eq!(
                        test_adapter_add(&next_publication, &next_generation, counter, 7)?,
                        7
                    );
                    assert_eq!(
                        test_adapter_value(&next_publication, &next_generation, counter)?,
                        7
                    );
                    println!(
                        "INTERNAL_PUBLICATION_RECOVERY_OK same_pid={} generation_changed=true old_effect_rejected=true",
                        original_pid
                    );
                }
                #[cfg(not(feature = "internal-publication"))]
                assert_eq!(counter.add(7)?, 7);
                assert_eq!(counter.label("count")?, "count: 7");
                println!("RIESEL_RECOVERY_OK");
            } else {
                return Err(io::Error::other("unexpected extra application restart").into());
            }
            Ok(())
        })
}
