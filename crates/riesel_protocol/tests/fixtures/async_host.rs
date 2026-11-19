//! Managed-process fixture for the native asynchronous component host.
//! Two interfaces share one provider-owned atomic state and Tokio listener.
use riesel_protocol::{
    AsyncRpcFuture, AsyncRpcHandler, AsyncRpcRouter, RpcError, run_async_tcp_rpc_host,
    run_component_from_env,
};
use riesel_schema::{
    CapabilityContract, CapabilityRef, OperationDefinition, OperationKind, ParameterDefinition,
    ProviderDefinition, TypeRef,
};
use serde_json::{Value, json};
use std::io;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

fn contract(id: &str, operation: &str, kind: OperationKind) -> CapabilityContract {
    CapabilityContract {
        capability: CapabilityRef {
            id: id.into(),
            version: 1,
        },
        operations: [(
            operation.into(),
            OperationDefinition {
                kind,
                description: format!("Exercise shared async fixture {operation}"),
                inputs: if operation == "add" {
                    vec![ParameterDefinition {
                        name: "amount".into(),
                        kind: TypeRef::U64,
                    }]
                } else {
                    vec![]
                },
                output: TypeRef::U64,
                error: TypeRef::String,
            },
        )]
        .into_iter()
        .collect(),
        types: Default::default(),
    }
}

struct Writer(Arc<AtomicU64>);

impl AsyncRpcHandler for Writer {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        Ok(contract(
            "example.async.writer",
            "add",
            OperationKind::Command,
        ))
    }

    fn dispatch<'a>(&'a self, operation: &'a str, arguments: Value) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            if operation != "add" {
                return Err(RpcError::UnknownOperation(operation.into()));
            }
            let amount = arguments
                .get("amount")
                .and_then(Value::as_u64)
                .ok_or_else(|| RpcError::InvalidArguments("missing amount".into()))?;
            let previous = self
                .0
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    value.checked_add(amount)
                })
                .map_err(|_| RpcError::InvalidArguments("counter overflow".into()))?;
            Ok(json!({"Ok": previous + amount}))
        })
    }
}

struct Observer(Arc<AtomicU64>);

impl AsyncRpcHandler for Observer {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        Ok(contract(
            "example.async.observer",
            "value",
            OperationKind::Query,
        ))
    }

    fn dispatch<'a>(&'a self, operation: &'a str, _: Value) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            if operation != "value" {
                return Err(RpcError::UnknownOperation(operation.into()));
            }
            Ok(json!({"Ok": self.0.load(Ordering::SeqCst)}))
        })
    }
}

fn main() -> io::Result<()> {
    let state = Arc::new(AtomicU64::new(0));
    let provider = ProviderDefinition {
        id: "example.async.managed".into(),
        capabilities: vec![
            Writer(Arc::clone(&state))
                .contract()
                .map_err(io::Error::other)?
                .capability,
            Observer(Arc::clone(&state))
                .contract()
                .map_err(io::Error::other)?
                .capability,
        ],
    };
    let router = AsyncRpcRouter::new(provider)
        .map_err(io::Error::other)?
        .route(Writer(Arc::clone(&state)))
        .map_err(io::Error::other)?
        .route(Observer(state))
        .map_err(io::Error::other)?;
    run_component_from_env(move |address| run_async_tcp_rpc_host(address, Arc::new(router)))
}
