//! Test-only app-process listener. This is not an approved SDK exporter or
//! secure public transport: loopback RPC has no caller authentication.
use riesel_protocol::{AsyncRpcHandler, serve_async_tcp_rpc_host};
use std::io::{self, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

pub async fn serve(handler: Arc<dyn AsyncRpcHandler>) -> io::Result<()> {
    // Research-only fixed-address restart probe. Default fixtures still use
    // an OS-assigned loopback port; no application discovery policy is implied.
    let bind_address = std::env::var("RIESEL_RESEARCH_FACADE_BIND")
        .unwrap_or_else(|_| "127.0.0.1:0".to_owned());
    let requested: std::net::SocketAddr = bind_address
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if !requested.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "research facade may only bind to loopback",
        ));
    }
    let listener = tokio::net::TcpListener::bind(requested).await?;
    let address = listener.local_addr()?;
    handler.validate_host().map_err(io::Error::other)?;
    let stopping = Arc::new(AtomicBool::new(false));
    let host = tokio::spawn(serve_async_tcp_rpc_host(
        listener,
        handler,
        Arc::clone(&stopping),
    ));

    // Socket bound, not an authenticated application-interface ready signal.
    println!("RESEARCH_FACADE_BOUND {address}");
    io::stdout().flush()?;

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
    Ok(())
}
