//! Thin executable host: all framing, readiness and graceful shutdown live
//! in the shared RPC runtime, not in Counter-specific networking code.

use crate::{MemoryCounter, memory_provider_definition};
use riesel_counter::CounterRpcDispatcher;
use riesel_protocol::{RpcHost, run_tcp_rpc_host};
use std::io;

pub fn run_counter_server(address: &str) -> io::Result<()> {
    let handler = RpcHost::new(
        memory_provider_definition().map_err(io::Error::other)?,
        CounterRpcDispatcher::new(MemoryCounter::default()),
    )
    .map_err(io::Error::other)?;
    run_tcp_rpc_host(address, &handler)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn rejects_non_loopback_counter_server() {
        let err = run_counter_server("0.0.0.0:0").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }
}
