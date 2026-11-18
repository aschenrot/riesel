//! Thin executable host: all framing, readiness and graceful shutdown live
//! in the shared RPC runtime, not in Counter-specific networking code.

use crate::{MemoryCounter, memory_provider_definition};
use riesel_counter::{CounterObserverRpcDispatcher, CounterRpcDispatcher};
use riesel_protocol::{RpcRouter, serve_router};
use std::io;

pub fn run_counter_server(address: &str) -> io::Result<()> {
    let state = std::sync::Arc::new(MemoryCounter::default());
    let router = RpcRouter::new(memory_provider_definition().map_err(io::Error::other)?)
        .and_then(|router| router.route(CounterRpcDispatcher::new(state.clone())))
        .and_then(|router| router.route(CounterObserverRpcDispatcher::new(state)))
        .map_err(io::Error::other)?;
    serve_router(address, router)
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
