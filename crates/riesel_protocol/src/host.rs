//! Standard managed component startup. Component binaries only supply
//! their implementation; argument parsing and runtime ownership are shared.
use crate::{RpcHandler, RpcHost, RpcRouter, run_tcp_rpc_host};
use riesel_schema::ProviderDefinition;
use std::ffi::OsString;
use std::io;
use std::net::SocketAddr;

/// Turn the provider metadata and a generated dispatcher into a hosted
/// capability. The shared TCP service owns readiness and shutdown.
pub fn serve_component<H: RpcHandler>(
    address: &str,
    provider: ProviderDefinition,
    handler: H,
) -> io::Result<()> {
    let hosted = RpcHost::new(provider, handler).map_err(io::Error::other)?;
    run_tcp_rpc_host(address, &hosted)
}

/// Host all capability bindings of one managed component through a shared
/// port, process and provider identity.
pub fn serve_router(address: &str, router: RpcRouter) -> io::Result<()> {
    router.validate_routes().map_err(io::Error::other)?;
    run_tcp_rpc_host(address, &router)
}

/// Run a native async host with the same managed process handshake as the
/// synchronous host. The runtime controls readiness and stdin shutdown;
/// Tokio owns nonblocking request tasks and cancels them on shutdown.
///
/// This is a low-level hosting entrypoint; component-generated registration
/// will call it once native async component wiring is implemented.
#[cfg(feature = "async-tcp")]
pub fn run_async_tcp_rpc_host(
    address: &str,
    handler: std::sync::Arc<dyn crate::AsyncRpcHandler>,
) -> io::Result<()> {
    use std::io::Write;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let address: SocketAddr = address.parse().map_err(io::Error::other)?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unauthenticated RPC services must listen on loopback",
        ));
    }
    handler.validate_host().map_err(io::Error::other)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::bind(address).await?;
        let controlled = std::env::var_os("RIESEL_CONTROL_STDIN").is_some();
        let stopping = Arc::new(AtomicBool::new(false));
        if controlled {
            let stopping = Arc::clone(&stopping);
            std::thread::spawn(move || {
                let mut line = String::new();
                if io::stdin().read_line(&mut line).is_ok() && line.trim_end() == "RIESEL_SHUTDOWN"
                {
                    stopping.store(true, Ordering::Release);
                }
            });
        }

        let ready = serde_json::json!({
            "protocol": 1,
            "address": listener.local_addr()?.to_string(),
            "shutdown": controlled.then_some("stdin_line")
        });
        println!("RIESEL_READY {}", ready);
        io::stdout().flush()?;

        crate::serve_async_tcp_rpc_host(listener, handler, stopping).await
    })
}

/// Let an executable accept an explicit bind address or the managed runtime's
/// RIESEL_BIND_ADDRESS environment setting without repeating startup code.
pub fn run_component_from_env(serve: impl FnOnce(&str) -> io::Result<()>) -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let address = parse_bind_address(&args, std::env::var_os("RIESEL_BIND_ADDRESS"))?;
    serve(&address.to_string())
}

fn parse_bind_address(args: &[OsString], env_bind: Option<OsString>) -> io::Result<SocketAddr> {
    if args.len() > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: <component> [bind-address] (or RIESEL_BIND_ADDRESS)",
        ));
    }
    let raw = args.first().cloned().or(env_bind).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "missing bind-address or RIESEL_BIND_ADDRESS",
        )
    })?;
    let raw = raw.into_string().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "bind-address is not valid UTF-8",
        )
    })?;
    raw.parse::<SocketAddr>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_address_overrides_runtime_default() {
        let address = parse_bind_address(
            &[OsString::from("127.0.0.1:40011")],
            Some(OsString::from("127.0.0.1:40122")),
        )
        .unwrap();
        assert_eq!(address.port(), 40011);
    }

    #[test]
    fn rejects_missing_or_extra_address_arguments() {
        assert!(parse_bind_address(&[], None).is_err());
        assert!(
            parse_bind_address(
                &[OsString::from("127.0.0.1:40001"), OsString::from("extra")],
                None
            )
            .is_err()
        );
        assert!(parse_bind_address(&[OsString::from("invalid")], None).is_err());
    }
}
