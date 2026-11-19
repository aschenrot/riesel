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
        receiver.await.map_err(|_| ApiError::Unavailable)?
    }
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
    use riesel::Application;
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

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
