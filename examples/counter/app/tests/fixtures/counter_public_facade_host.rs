//! Research-only application-owned typed interface. This is deliberately NOT
//! a supported public exporter: existing loopback RPC lacks caller auth and
//! the app must manually own this endpoint and its lifecycle.
use riesel::Application;
use riesel_counter::AsyncCounterAsyncRpcDispatcher;
use riesel_counter_component::AsyncMemoryCounter;
use riesel_protocol::{AsyncRpcHandler, serve_async_tcp_rpc_host};
use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

fn main() -> riesel::Result<()> {
    Application::new("PublicFacadeResearch")
        .async_handler(|| async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            // Only this explicit public facade is served. The existing
            // Observer capability is intentionally not registered here.
            let handler = Arc::new(AsyncCounterAsyncRpcDispatcher::new(
                AsyncMemoryCounter::default(),
            ));
            handler.validate_host().map_err(io::Error::other)?;
            let stopping = Arc::new(AtomicBool::new(false));
            let host = tokio::spawn(serve_async_tcp_rpc_host(
                listener,
                handler,
                Arc::clone(&stopping),
            ));

            // Means socket bound, NOT proof of a usable/authenticated API.
            println!("RESEARCH_FACADE_BOUND {address}");
            io::stdout().flush()?;

            // Test-only lifecycle control from the fixture owner. A product
            // exporter would need session generation and ownership semantics.
            let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
            std::thread::spawn(move || {
                let mut line = String::new();
                if io::stdin().read_line(&mut line).is_ok() && line.trim() == "STOP" {
                    let _ = stop_tx.send(());
                }
            });
            stop_rx
                .await
                .map_err(|_| io::Error::other("facade control stdin closed"))?;
            stopping.store(true, Ordering::Release);
            tokio::time::timeout(Duration::from_secs(5), host)
                .await
                .map_err(io::Error::other)?
                .map_err(io::Error::other)??;
            Ok::<(), io::Error>(())
        })?
        .run()
}
