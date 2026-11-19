pub mod host;
pub mod rpc;
pub mod session;
pub mod tcp;

pub use rpc::{
    AsyncRpcClientBinding, AsyncRpcFuture, AsyncRpcHandler, AsyncRpcRouter, AsyncRpcTransport,
    CallError, ComponentAdapter, ComponentHandler, LocalBindingMode, LocalRpc, RpcClientBinding,
    RpcError, RpcHandler, RpcHost, RpcRouter, RpcTransport, decode_result,
};
#[cfg(feature = "async-tcp")]
pub use tcp::{AsyncTcpRpcTransport, handle_async_tcp_connection, serve_async_tcp_rpc_host};
pub use tcp::{TcpRpcTransport, handle_tcp_connection, run_tcp_rpc_host};

pub use session::SessionToken;

#[cfg(feature = "async-tcp")]
pub use host::run_async_tcp_rpc_host;
pub use host::{run_component_from_env, serve_component};

pub use host::serve_router;
