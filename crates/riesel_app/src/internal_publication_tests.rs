//! #157 feature-gated runtime ownership acceptance: an admitted publication
//! operation must be owned by the existing application AsyncTasks scope.
//! No external caller authentication, exported interface or credential here.
use crate::{App, ConnectorError, ConnectorRegistry, RunningApp, RuntimeError};
use riesel_protocol::session::{InternalAdmissionError, InternalDrain};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

fn start_runtime_session() -> RunningApp {
    App::from_schema(r#"{"version":1,"app":{"name":"OwnedIngress"},"models":{},"requires":[]}"#)
        .unwrap()
        .start_local(&ConnectorRegistry::new())
        .unwrap()
}

#[test]
fn admitted_async_work_is_joined_before_new_app_session_and_ordinary_stop() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let mut first = start_runtime_session();
        let old_session = first.session_token();
        let first_publication = old_session.internal_publication_session();
        let old_generation = first_publication.generation();
        let tasks = first.async_tasks();
        tasks.begin_scope().unwrap();

        let effects = Arc::new(AtomicUsize::new(0));
        let attempted_effects = Arc::clone(&effects);
        let first_worker = first_publication.clone();
        let expected = old_generation.clone();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        tasks
            .spawn("owned-inflight-request", async move {
                // A real runtime-owned task holds its ingress permit for the
                // whole dispatched future, including its pending duration.
                let _permit = first_worker.try_admit(&expected).unwrap();
                entered_tx.send(()).unwrap();
                std::future::pending::<()>().await;
                attempted_effects.fetch_add(1, Ordering::SeqCst);
                Ok::<(), ConnectorError>(())
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first_publication.in_flight(), 1);
        assert_eq!(effects.load(Ordering::SeqCst), 0);

        // Normal async app stop must drop and join the owned worker while
        // closing admission. It must not wait indefinitely for a pending
        // future or drop its permit before the request future ends.
        tokio::time::timeout(Duration::from_secs(2), tasks.finish_scope())
            .await
            .expect("application-owned request must be cancelled and joined")
            .unwrap();
        assert_eq!(effects.load(Ordering::SeqCst), 0);
        assert_eq!(first_publication.in_flight(), 0);
        assert_eq!(
            first_publication.drain_for(Duration::from_secs(1)).await,
            InternalDrain::Drained
        );
        assert_eq!(
            first_publication.try_admit(&old_generation).unwrap_err(),
            InternalAdmissionError::Expired
        );
        assert!(matches!(
            tasks.spawn("late-request", async { Ok::<(), ConnectorError>(()) }),
            Err(RuntimeError::TaskGroupClosed)
        ));
        first.shutdown().unwrap();

        // Same OS process, new actual runtime; neither an old attachment nor
        // an old worker can be promoted into the successor application.
        let mut second = start_runtime_session();
        let second_publication = second.session_token().internal_publication_session();
        let second_generation = second_publication.generation();
        assert_ne!(old_generation, second_generation);
        assert_eq!(
            second_publication.try_admit(&old_generation).unwrap_err(),
            InternalAdmissionError::WrongGeneration
        );
        assert_eq!(
            first_publication.try_admit(&old_generation).unwrap_err(),
            InternalAdmissionError::Expired
        );

        let current_tasks = second.async_tasks();
        current_tasks.begin_scope().unwrap();
        let current_publication = second_publication.clone();
        let current_effects = Arc::clone(&effects);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        current_tasks
            .spawn("fresh-owned-request", async move {
                let _permit = current_publication.try_admit(&second_generation).unwrap();
                current_effects.fetch_add(1, Ordering::SeqCst);
                done_tx.send(()).unwrap();
                Ok::<(), ConnectorError>(())
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), done_rx)
            .await
            .unwrap()
            .unwrap();
        current_tasks.finish_scope().await.unwrap();
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        assert_eq!(second_publication.in_flight(), 0);
        second.shutdown().unwrap();
    });
}
