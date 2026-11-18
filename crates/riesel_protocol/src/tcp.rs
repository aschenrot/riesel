//! Bounded JSON-line TCP transport for generated capability RPC.
//! This first transport is **loopback-only** and has no authentication.

use crate::{RpcError, RpcHandler, RpcTransport, SessionToken};
use riesel_schema::{CapabilityContract, CapabilityRef, ProviderDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex, PoisonError, mpsc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;


mod client;
mod server;
mod wire;
#[cfg(test)]
mod tests;

pub use client::TcpRpcTransport;
pub use server::{handle_tcp_connection, run_tcp_rpc_host};

use wire::{
    DESCRIBE, PROVIDER, WIRE_VERSION, WireRequest, WireResponse, configure_timeouts,
    read_json_line, write_json_line,
};
#[cfg(test)]
use server::serve_connections;
