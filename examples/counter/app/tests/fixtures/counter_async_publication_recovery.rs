//! Real #157 end-to-end acceptance: one SDK-owned async session, a real
//! managed Counter process crash, an owned admitted worker, and recovery.
//! No public listener, trusted peer, grant, transport or exporter is created.
use riesel::{Application, AsyncCapability, AsyncTasks, ConnectorError};
use riesel_counter::CounterContract;
use riesel_protocol::session::{
    InternalAdmissionError, InternalPublicationPermit, InternalPublicationSession,
};
use std::io;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

struct TestAdapter {
    publication: InternalPublicationSession,
    published: Mutex<bool>,
}

impl TestAdapter {
    fn new(publication: InternalPublicationSession) -> Self {
        Self {
            publication,
            published: Mutex::new(true),
        }
    }

    // A short, adapter-local lock serializes withdrawal with new admissions.
    // Neither this lock nor the runtime ingress lock surrounds user code.
    fn admit(&self, generation: &str) -> io::Result<InternalPublicationPermit> {
        let published = self.published.lock().unwrap();
        if !*published {
            return Err(io::Error::other("test adapter has been withdrawn"));
        }
        self.publication
            .try_admit(generation)
            .map_err(|error| io::Error::other(format!("internal admission: {error:?}")))
    }

    fn withdraw(&self) {
        *self.published.lock().unwrap() = false;
    }
}

struct MarkDropped(Arc<AtomicBool>);

impl Drop for MarkDropped {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

struct Previous {
    adapter: Arc<TestAdapter>,
    generation: String,
    process_id: u32,
    owned_worker_dropped: Arc<AtomicBool>,
    // Deliberately keep the request pending until Riesel cancels its owner.
    // Dropping this sender prematurely would make the test vacuous.
    _pending_sender: tokio::sync::oneshot::Sender<()>,
}

fn main() -> riesel::Result<()> {
    let state = Arc::new(Mutex::new(None::<Previous>));
    let mut executions = 0;

    Application::new("AsyncPublicationRecovery")
        .async_handler_with_aliases(
            move |counter: AsyncCapability<CounterContract>, tasks: AsyncTasks| {
                executions += 1;
                let execution = executions;
                let state = Arc::clone(&state);
                async move {
                    let publication = tasks.internal_publication_session();
                    let generation = publication.generation();
                    let adapter = Arc::new(TestAdapter::new(publication));

                    if execution == 1 {
                        // The actual selected managed Counter provider is
                        // callable before publication, but publication is a
                        // distinct explicit application-owned boundary.
                        assert_eq!(counter.value().await?, 0);
                        {
                            let _permit = adapter.admit(&generation)?;
                            assert_eq!(counter.add(2).await?, 2);
                        }

                        let dropped = Arc::new(AtomicBool::new(false));
                        let owned_dropped = Arc::clone(&dropped);
                        let worker_adapter = Arc::clone(&adapter);
                        let worker_counter = counter.clone();
                        let worker_generation = generation.clone();
                        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
                        let (pending_tx, pending_rx) = tokio::sync::oneshot::channel::<()>();

                        // Crucially, a real app-owned async request runs on
                        // Riesel's existing scoped executor, not tokio::spawn.
                        // The permit remains alive across the provider read
                        // and all subsequent pending async work.
                        tasks.spawn("counter-admitted-pending", async move {
                            let _permit = worker_adapter.admit(&worker_generation)?;
                            let _drop_mark = MarkDropped(owned_dropped);
                            assert_eq!(worker_counter.value().await?, 2);
                            let _ = entered_tx.send(());
                            let _ = pending_rx.await;
                            Ok::<(), ConnectorError>(())
                        })?;

                        tokio::time::timeout(Duration::from_secs(5), entered_rx)
                            .await
                            .map_err(|_| io::Error::other("owned Counter worker never admitted"))?
                            .map_err(|_| io::Error::other("owned Counter worker exited before admission"))?;
                        assert_eq!(adapter.publication.in_flight(), 1);
                        assert!(!dropped.load(Ordering::Acquire));

                        *state.lock().unwrap() = Some(Previous {
                            adapter,
                            generation,
                            process_id: std::process::id(),
                            owned_worker_dropped: dropped,
                            _pending_sender: pending_tx,
                        });

                        // The managed fixture now crashes its first child.
                        // Only a real supervised session failure may wake this
                        // handler and trigger eligible whole-app recovery.
                        tokio::time::timeout(
                            Duration::from_secs(10),
                            std::future::pending::<()>(),
                        )
                        .await
                        .map_err(|_| io::Error::other("managed Counter did not crash"))?;
                        unreachable!("pending future never completes");
                    } else if execution == 2 {
                        let old = state.lock().unwrap().take().expect("session A must exist");
                        assert_eq!(std::process::id(), old.process_id);
                        assert_ne!(old.generation, generation);
                        assert!(
                            old.owned_worker_dropped.load(Ordering::Acquire),
                            "old runtime must join/cancel its admitted async request"
                        );
                        assert_eq!(old.adapter.publication.in_flight(), 0);
                        assert_eq!(
                            old.adapter
                                .publication
                                .try_admit(&old.generation)
                                .unwrap_err(),
                            InternalAdmissionError::Expired
                        );
                        assert_eq!(
                            adapter.publication.try_admit(&old.generation).unwrap_err(),
                            InternalAdmissionError::WrongGeneration
                        );

                        // The old adapter is still in scope, but it cannot
                        // mutate the replacement Counter (pre-effect fence).
                        assert!(old.adapter.admit(&old.generation).is_err());
                        assert_eq!(counter.value().await?, 0);
                        {
                            let _permit = adapter.admit(&generation)?;
                            assert_eq!(counter.add(7).await?, 7);
                        }

                        // Explicit adapter withdrawal is independent from
                        // whole-app invalidation. In-flight work may complete,
                        // no new admission succeeds, and the selected Counter
                        // remains alive for its owning application.
                        let _held = adapter.admit(&generation)?;
                        adapter.withdraw();
                        assert!(adapter.admit(&generation).is_err());
                        assert_eq!(counter.value().await?, 7);
                        assert_eq!(adapter.publication.in_flight(), 1);
                        drop(_held);
                        assert_eq!(adapter.publication.in_flight(), 0);
                        println!(
                            "INTERNAL_ASYNC_MANAGED_RECOVERY_OK same_pid={} generation_changed=true owned_worker_cancelled=true stale_effect_rejected=true adapter_withdrawn=true",
                            old.process_id
                        );
                    } else {
                        return Err(io::Error::other("unexpected extra session").into());
                    }
                    Ok::<(), ConnectorError>(())
                }
            },
            ["counter", "tasks"],
        )?
        .run_from_env()?;
    Ok(())
}
