pub mod rpc;

pub use rpc::{
    CallError, LocalRpc, RpcError, RpcHandler, RpcTransport, decode_result,
};

pub enum Request {
    Launch { app_id: String },
    Status { instance_id: String },
    Stop { instance_id: String },
}
