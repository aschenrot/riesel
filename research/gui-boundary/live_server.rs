//! Read-only, loopback-only browser adapter for research #248.
//! This is NOT authenticated attachment, a public exporter, or safe for sensitive state.
use axum::{
    Json, Router,
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use riesel::{ExecutionGraphRunner, GraphRunReport, GraphSignalSender};
use serde_json::Value;
use std::{io::{self, Write}, sync::Arc, time::Duration};

pub type SnapshotSource = Arc<dyn Fn() -> io::Result<Value> + Send + Sync>;

pub async fn run(
    runner: ExecutionGraphRunner,
    signal: GraphSignalSender,
    source: SnapshotSource,
    linger_seconds: u64,
) -> riesel::Result<GraphRunReport> {
    // The OS selects the port. Never bind on a public interface.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let expected_host = address.to_string();

    let snapshot = get({
        let source = Arc::clone(&source);
        move || {
            let source = Arc::clone(&source);
            async move {
                match source() {
                    Ok(document) => Json(document).into_response(),
                    Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
                }
            }
        }
    });
    let app = Router::new()
        .route("/", get(|| async { Html(include_str!("inspector.html")) }))
        .route("/inspector.js", get(|| async {
            ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")],
                include_str!("inspector.js"))
        }))
        .route("/snapshot", snapshot)
        .layer(middleware::from_fn(move |request: Request<axum::body::Body>, next: Next| {
            let expected_host = expected_host.clone();
            async move {
                // Refuse DNS rebinding and non-local host aliases. Same-origin
                // browser policy prevents foreign origins from reading JSON;
                // neither protection authenticates another local OS process.
                if request.headers().get(header::HOST)
                    .and_then(|value| value.to_str().ok())
                    != Some(expected_host.as_str())
                {
                    return StatusCode::MISDIRECTED_REQUEST.into_response();
                }
                if request.method() != Method::GET && request.method() != Method::HEAD {
                    return StatusCode::METHOD_NOT_ALLOWED.into_response();
                }
                let mut response: Response = next.run(request).await;
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    HeaderValue::from_static("no-store, max-age=0"),
                );
                response.headers_mut().insert(
                    header::CONTENT_SECURITY_POLICY,
                    HeaderValue::from_static(
                        "default-src 'none'; script-src 'self'; connect-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'",
                    ),
                );
                response.headers_mut().insert(
                    header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"),
                );
                response
            }
        }));
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async { let _ = stop_rx.await; })
            .await
    });

    println!("RESEARCH LIVE INSPECTOR: http://{address}/");
    println!("Read-only, unauthenticated local proof. Ctrl+C stops the application; no UI controls are exposed.");
    io::stdout().flush()?;

    // The app, not the GUI, controls admission. Allow time for a browser to
    // attach before beginning the bounded sample workflow.
    tokio::time::sleep(Duration::from_secs(3)).await;
    signal.fire("start")?;
    let report = runner.execute().await;

    // Preserve the finished authoritative snapshot long enough to inspect.
    // A browser closing or reopening does not affect this application run.
    tokio::time::sleep(Duration::from_secs(linger_seconds)).await;
    let _ = stop_tx.send(());
    server.await.map_err(io::Error::other)??;
    Ok(report)
}
