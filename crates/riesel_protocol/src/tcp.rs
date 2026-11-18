//! Bounded JSON-line TCP transport for generated capability RPC.
//! This first transport is **loopback-only** and has no authentication.

use crate::{RpcError, RpcHandler, RpcTransport, SessionToken};
use riesel_schema::{CapabilityContract, CapabilityRef, ProviderDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

mod client;
mod server;
#[cfg(test)]
mod tests;
mod wire;

pub use client::TcpRpcTransport;
pub use server::{handle_tcp_connection, run_tcp_rpc_host};

#[cfg(test)]
use server::serve_connections;
use wire::{
    DESCRIBE, PROVIDER, WIRE_VERSION, WireRequest, WireResponse, configure_timeouts,
    read_json_line, read_json_line_optional, write_json_line,
};
