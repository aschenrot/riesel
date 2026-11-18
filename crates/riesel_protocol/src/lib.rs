pub mod rpc;
pub mod tcp;

pub use rpc::{CallError, LocalRpc, RpcError, RpcHandler, RpcTransport, decode_result};
pub use tcp::{TcpRpcTransport, handle_tcp_connection};

pub enum Request {
    Launch { app_id: String },
    Status { instance_id: String },
    Stop { instance_id: String },
}
