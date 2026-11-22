//! Todo-only local HTTP adapter. No generic Riesel web capability is added.
mod assets;
mod routes;
mod security;

use crate::{TaskData, Todo, TodoError};
use riesel::{AsyncTasks, Records};
use routes::ApiError;
use std::future::IntoFuture;
use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone)]
struct HttpState {
    records: Records<TaskData>,
    tasks: AsyncTasks,
    authority: Arc<str>,
    next_request: Arc<AtomicU64>,
}

impl HttpState {
    async fn invoke<R, F>(&self, operation: F) -> Result<R, ApiError>
    where
        R: Send + 'static,
        F: FnOnce(Todo) -> Result<R, TodoError> + Send + 'static,
    {
        let records = self.records.clone();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let name = format!(
            "todo-http-{}",
            self.next_request.fetch_add(1, Ordering::Relaxed)
        );
        self.tasks
            .spawn_blocking(name, move |cancel| {
                if cancel.is_cancelled() {
                    let _ = sender.send(Err(ApiError::Unavailable));
                    return Ok(());
                }
                // A domain error is one HTTP response, not a fatal runtime task.
                let result = operation(Todo::new(records)).map_err(ApiError::from);
                let _ = sender.send(result);
                Ok(())
            })
            .map_err(|_| ApiError::Unavailable)?;
        receive_owned_result(receiver).await
    }
}

/// An accepted blocking operation may have changed persistent data even if
/// its result sender disappears (worker failure, cancellation or teardown).
/// Unlike synchronous task-admission refusal, this outcome is ambiguous.
/// Never automatically replay the command merely because its reply was lost.
async fn receive_owned_result<R>(
    receiver: tokio::sync::oneshot::Receiver<Result<R, ApiError>>,
) -> Result<R, ApiError> {
    receiver.await.map_err(|_| ApiError::OutcomeUnknown)?
}

fn router(state: HttpState, directory: Option<&std::path::Path>) -> axum::Router {
    use axum::{Router, extract::DefaultBodyLimit, middleware, routing::get};

    assets::mount(
        Router::new()
            .route("/api/v1/tasks", get(routes::list).post(routes::create))
            .route(
                "/api/v1/tasks/{id}",
                get(routes::show)
                    .patch(routes::patch)
                    .delete(routes::delete),
            ),
        directory,
    )
    .method_not_allowed_fallback(|| async { ApiError::MethodNotAllowed })
    .layer(DefaultBodyLimit::max(16 * 1024))
    .layer(middleware::from_fn_with_state(
        state.clone(),
        security::gate,
    ))
    .with_state(state)
}

/// Serve on numeric loopback only. No authentication or remote deployment is
/// promised; the same-origin browser defense is implemented in security.
pub async fn serve(
    records: Records<TaskData>,
    tasks: AsyncTasks,
    port: u16,
    directory: Option<PathBuf>,
) -> io::Result<()> {
    let directory = assets::verify(directory)?;
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    let addr = listener.local_addr()?;
    let state = HttpState {
        records,
        tasks,
        authority: Arc::from(format!("127.0.0.1:{}", addr.port())),
        next_request: Arc::new(AtomicU64::new(1)),
    };
    println!("TODO_HTTP_READY {addr}");
    io::stdout().flush()?;

    let (stopping, observed) = tokio::sync::oneshot::channel::<()>();
    let signal = async move {
        let _ = tokio::signal::ctrl_c().await;
        let _ = stopping.send(());
    };
    let server = axum::serve(listener, router(state, directory.as_deref()))
        .with_graceful_shutdown(signal)
        .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result?,
        _ = observed => {
            // Start the deadline *after* Ctrl-C, not at server startup.
            match tokio::time::timeout(std::time::Duration::from_secs(5), &mut server).await {
                Ok(result) => result?,
                Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, "HTTP shutdown exceeded five seconds")),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;
    use riesel::Application;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    /// A real persistent Todo/SQLite application owns one real local socket.
    /// The adapter can attest that *its own* listener bound, but cannot
    /// authenticate a client or claim that an operation is callable.
    #[cfg(feature = "internal-observation")]
    #[test]
    fn real_todo_sqlite_listener_has_only_adapter_evidence() {
        use riesel_app::operational_observation::{
            ObservationSnapshot, ObservationSource, ObservationState, ObservationSubject,
            MAX_OBSERVATION_AGE,
        };
        use std::sync::Mutex;

        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("observed-todo.sqlite");
        let captured = Arc::new(Mutex::new(None::<ObservationSnapshot>));
        let for_handler = Arc::clone(&captured);

        Application::new("TodoObservation")
            .async_handler(move |records: Records<TaskData>, tasks: AsyncTasks| {
                let captured = Arc::clone(&for_handler);
                async move {
                    // The actual typed Todo command commits to linked SQLite.
                    let task = Todo::new(records)
                        .create_task("Observed persistent task")
                        .map_err(io::Error::other)?;
                    assert_eq!(task.value.title, "Observed persistent task");

                    // A real bound socket is evidence of an adapter-owned
                    // listener event, not authorized HTTP dispatch.
                    let listener =
                        tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
                    let address = listener.local_addr()?;
                    let observed = ObservationSnapshot::local_listener(
                        tasks.internal_session_token(),
                        "todo.http",
                        address,
                    )
                    .map_err(io::Error::other)?;
                    assert_eq!(
                        observed.state_of(
                            &ObservationSubject::LocalApplicationListener {
                                interface: "todo.http".into(),
                                address,
                            },
                            std::time::Instant::now()
                        ),
                        ObservationState::Observed
                    );
                    assert_eq!(
                        observed.facts()[0].source,
                        ObservationSource::ApplicationAdapter
                    );
                    assert_eq!(
                        observed.state_of(
                            &ObservationSubject::AuthorizedApplicationOperation,
                            std::time::Instant::now()
                        ),
                        ObservationState::Unsupported
                    );
                    assert_eq!(
                        observed.state_of(
                            &ObservationSubject::OsLaunchAttempt,
                            std::time::Instant::now()
                        ),
                        ObservationState::Unsupported
                    );

                    // A real TCP handshake still does not supply any
                    // application grant; the test deliberately dispatches
                    // no Todo HTTP API operation through the bare socket.
                    let client = tokio::net::TcpStream::connect(address).await?;
                    let (accepted, _) = listener.accept().await?;
                    drop(accepted);
                    drop(client);
                    drop(listener);
                    assert_eq!(
                        observed.state_at(
                            0,
                            observed.observed_at()
                                + MAX_OBSERVATION_AGE
                                + Duration::from_nanos(1)
                        ),
                        Some(ObservationState::Stale),
                        "an old bind announcement must age out"
                    );
                    *captured.lock().unwrap() = Some(observed);
                    Ok::<(), io::Error>(())
                }
            })
            .unwrap()
            .sqlite_storage(database.clone())
            .unwrap()
            .run()
            .unwrap();

        let old = captured.lock().unwrap().take().unwrap();
        assert_eq!(
            old.state_at(0, std::time::Instant::now()),
            Some(ObservationState::Stale),
            "the adapter-owned observation must expire on real runtime shutdown"
        );

        // A new Todo runtime opens exactly the same persisted records. It is
        // not allowed to promote the old interface observation or session.
        Application::new("TodoObservationReopen")
            .handler(|records: Records<TaskData>| -> Result<(), TodoError> {
                let rows = Todo::new(records).list_tasks()?;
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].value.title, "Observed persistent task");
                Ok(())
            })
            .unwrap()
            .sqlite_storage(database)
            .unwrap()
            .run()
            .unwrap();
    }

    /// HTTP requests cannot bypass the runtime's eight owned blocking slots.
    /// In particular, denied operations must not execute after returning 503.
    #[test]
    fn exhausted_blocking_budget_rejects_request_without_executing_domain() {
        Application::new("Todo HTTP blocking admission")
            .async_handler(|records: Records<TaskData>, tasks: AsyncTasks| async move {
                for number in 0..8 {
                    tasks
                        .spawn_blocking(format!("occupied-{number}"), |cancel| {
                            let _ = cancel.wait_cancelled_timeout(Duration::from_secs(5));
                            Ok(())
                        })
                        .unwrap();
                }
                let state = HttpState {
                    records,
                    tasks,
                    authority: Arc::from("127.0.0.1:0"),
                    next_request: Arc::new(AtomicU64::new(1)),
                };
                let executed = Arc::new(AtomicBool::new(false));
                let observed = Arc::clone(&executed);
                let result = state
                    .invoke(move |todo| {
                        observed.store(true, Ordering::SeqCst);
                        todo.list_tasks()
                    })
                    .await;
                assert!(matches!(result, Err(ApiError::Unavailable)));
                assert!(!executed.load(Ordering::SeqCst));
                Ok::<(), std::io::Error>(())
            })
            .unwrap()
            .development()
            .unwrap()
            .run()
            .unwrap();
    }

    /// A real accepted Todo mutation can commit before its reply disappears.
    /// The HTTP outcome must be explicitly unknown rather than a definite
    /// pre-dispatch capacity rejection, and must never be replayed.
    #[test]
    fn lost_reply_after_committed_todo_write_is_outcome_unknown() {
        Application::new("Todo HTTP lost response after effect")
            .async_handler(|records: Records<TaskData>, tasks: AsyncTasks| async move {
                let writer = records.clone();
                let (sender, receiver) = tokio::sync::oneshot::channel::<Result<(), ApiError>>();
                tasks.spawn_blocking("committed-lost-reply", move |_cancellation| {
                    Todo::new(writer).create_task("Applied exactly once")?;
                    // The worker completed the real Storage operation but
                    // did not deliver a response. Do not infer rollback.
                    drop(sender);
                    Ok(())
                })?;
                assert!(matches!(
                    receive_owned_result(receiver).await,
                    Err(ApiError::OutcomeUnknown)
                ));
                let tasks_after = Todo::new(records).list_tasks()?;
                assert_eq!(tasks_after.len(), 1);
                assert_eq!(tasks_after[0].value.title, "Applied exactly once");

                let response = ApiError::OutcomeUnknown.into_response();
                assert_eq!(
                    response.status(),
                    axum::http::StatusCode::SERVICE_UNAVAILABLE
                );
                let body = axum::body::to_bytes(response.into_body(), 1024).await?;
                let json: serde_json::Value = serde_json::from_slice(&body)?;
                assert_eq!(json["error"]["code"], "outcome_unknown");
                assert_eq!(
                    json["error"]["message"],
                    "Request outcome could not be confirmed"
                );
                Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
            })
            .unwrap()
            .development()
            .unwrap()
            .run()
            .unwrap();
    }

    /// A runtime task fault revokes the session while the HTTP server is
    /// pending. The listener is dropped and owned workers are joined.
    #[test]
    fn unexpected_session_revocation_releases_listener() {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);

        let result = Application::new("Todo HTTP revoked session")
            .async_handler(
                move |records: Records<TaskData>, tasks: AsyncTasks| async move {
                    tasks.spawn("simulate-fatal-provider-work", async {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        Err(std::io::Error::other("fatal session worker").into())
                    })?;
                    serve(records, tasks, port, None).await?;
                    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
                },
            )
            .unwrap()
            .development()
            .unwrap()
            .run();
        assert!(result.is_err(), "fatal scoped task must stop the app");
        assert!(
            std::net::TcpListener::bind(("127.0.0.1", port)).is_ok(),
            "revoked session must release its listener"
        );
    }
}
