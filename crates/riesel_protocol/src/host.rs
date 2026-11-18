//! Standard managed component startup. Component binaries only supply
//! their implementation; argument parsing and runtime ownership are shared.
use crate::{RpcHandler, RpcHost, run_tcp_rpc_host};
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
