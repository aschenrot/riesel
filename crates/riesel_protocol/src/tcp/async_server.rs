//! Bounded, nonblocking TCP hosting for generated native async handlers.
//! This is a transport primitive, not yet Riesel's supervised app lifecycle.

use super::*;
use crate::AsyncRpcHandler;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener as AsyncTcpListener, TcpStream as AsyncTcpStream};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

const MAX_ACTIVE_CONNECTIONS: usize = 32;
const IDLE_ACCEPT_POLL: Duration = Duration::from_millis(25);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

async fn read_request(stream: &mut AsyncTcpStream) -> Result<WireRequest, RpcError> {
    let mut frame = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = stream
            .read(&mut buffer)
            .await
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        if read == 0 {
            return Err(RpcError::Transport(
                "RPC connection closed before complete frame".into(),
            ));
        }
        let newline = buffer[..read].iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(read, |index| index + 1);
        if frame.len() + length > wire::MAX_FRAME_BYTES {
            return Err(RpcError::Transport("RPC frame is too large".into()));
        }
        frame.extend_from_slice(&buffer[..length]);
        if newline.is_some() {
            return serde_json::from_slice(&frame)
                .map_err(|error| RpcError::Codec(error.to_string()));
        }
    }
}

async fn respond(
    handler: &(dyn AsyncRpcHandler + 'static),
    request: WireRequest,
) -> Result<Value, RpcError> {
    if request.version != WIRE_VERSION {
        return Err(RpcError::Transport(format!(
            "unsupported RPC request protocol version: {}",
            request.version
        )));
    }
    let contract = handler.contract_for(&request.capability)?;
    if request.operation == DESCRIBE || request.operation == PROVIDER {
        if request.arguments != serde_json::json!({}) {
            return Err(RpcError::InvalidArguments(
                "RPC inspection takes no arguments".into(),
            ));
        }
        if request.operation == DESCRIBE {
            return serde_json::to_value(contract)
                .map_err(|error| RpcError::Codec(error.to_string()));
        }
        let provider = handler
            .provider()
            .ok_or_else(|| RpcError::Contract("service has no registered provider".into()))?;
        return serde_json::to_value(provider).map_err(|error| RpcError::Codec(error.to_string()));
    }
    handler
        .dispatch_for(&request.capability, &request.operation, request.arguments)
        .await
}

/// Execute exactly one framed request over a Tokio TCP socket. The caller
/// owns connection admission, cancellation and listener lifecycle.
pub async fn handle_async_tcp_connection(
    handler: &(dyn AsyncRpcHandler + 'static),
    stream: &mut AsyncTcpStream,
) -> Result<(), RpcError> {
    timeout(REQUEST_TIMEOUT, async {
        let request = read_request(stream).await?;
        let result = respond(handler, request).await;
        let mut frame = serde_json::to_vec(&WireResponse {
            version: WIRE_VERSION,
            result,
        })
        .map_err(|error| RpcError::Codec(error.to_string()))?;
        if frame.len() + 1 > wire::MAX_FRAME_BYTES {
            return Err(RpcError::Transport("RPC response is too large".into()));
        }
        frame.push(b'\n');
        stream
            .write_all(&frame)
            .await
            .map_err(|error| RpcError::Transport(error.to_string()))
    })
    .await
    .map_err(|_| RpcError::Transport("RPC request timed out".into()))?
}

/// Listen on loopback only, cap active requests and own every spawned task.
/// Setting `stopping` stops admission and cancels pending request futures.
/// This does not guarantee that a partially executed command was rolled back.
pub async fn serve_async_tcp_rpc_host(
    listener: AsyncTcpListener,
    handler: Arc<dyn AsyncRpcHandler>,
    stopping: Arc<AtomicBool>,
) -> io::Result<()> {
    if !listener.local_addr()?.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unauthenticated RPC services must listen on loopback",
        ));
    }
    handler.validate_host().map_err(io::Error::other)?;

    let permits = Arc::new(Semaphore::new(MAX_ACTIVE_CONNECTIONS));
    let mut tasks = JoinSet::new();
    let mut result = Ok(());
    while !stopping.load(Ordering::Acquire) {
        match timeout(IDLE_ACCEPT_POLL, listener.accept()).await {
            Ok(Ok((mut stream, _))) => {
                let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
                    // Refuse overload rather than building an unbounded
                    // pending connection queue.
                    continue;
                };
                let handler = Arc::clone(&handler);
                tasks.spawn(async move {
                    let _permit = permit;
                    let _ = handle_async_tcp_connection(handler.as_ref(), &mut stream).await;
                });
            }
            Ok(Err(error)) => {
                result = Err(error);
                break;
            }
            Err(_) => {}
        }

        while let Some(joined) = tasks.try_join_next() {
            if joined.is_err() {
                result = Err(io::Error::other("async RPC worker panicked"));
                break;
            }
        }
        if result.is_err() {
            break;
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AsyncRpcFuture, AsyncRpcTransport};
    use riesel_schema::{OperationDefinition, OperationKind, TypeRef};

    struct ExampleHandler;

    impl AsyncRpcHandler for ExampleHandler {
        fn contract(&self) -> Result<CapabilityContract, RpcError> {
            Ok(CapabilityContract {
                capability: CapabilityRef {
                    id: "example.async".into(),
                    version: 1,
                },
                operations: [(
                    "value".into(),
                    OperationDefinition {
                        kind: OperationKind::Query,
                        description: "Read an asynchronous value".into(),
                        inputs: vec![],
                        output: TypeRef::U64,
                        error: TypeRef::String,
                    },
                )]
                .into_iter()
                .collect(),
                types: Default::default(),
            })
        }

        fn dispatch<'a>(&'a self, operation: &'a str, _: Value) -> AsyncRpcFuture<'a> {
            Box::pin(async move {
                if operation == "value" {
                    Ok(serde_json::json!({"Ok": 7}))
                } else {
                    Err(RpcError::UnknownOperation(operation.into()))
                }
            })
        }
    }

    #[test]
    fn async_host_serves_contract_and_calls_without_blocking_threads() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = AsyncTcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let stopping = Arc::new(AtomicBool::new(false));
            let host = tokio::spawn(serve_async_tcp_rpc_host(
                listener,
                Arc::new(ExampleHandler),
                Arc::clone(&stopping),
            ));

            let client = AsyncTcpRpcTransport::new(address);
            let expected = ExampleHandler.contract().unwrap();
            client.verify(&expected).await.unwrap();
            let value = client
                .call(&expected.capability, "value", serde_json::json!({}))
                .await
                .unwrap();
            assert_eq!(value, serde_json::json!({"Ok": 7}));
            assert!(matches!(
                client
                    .call(&expected.capability, "missing", serde_json::json!({}))
                    .await,
                Err(RpcError::UnknownOperation(_))
            ));

            stopping.store(true, Ordering::Release);
            timeout(Duration::from_secs(2), host)
                .await
                .expect("async RPC host did not stop")
                .unwrap()
                .unwrap();
        });
    }
    struct SecondHandler;

    impl AsyncRpcHandler for SecondHandler {
        fn contract(&self) -> Result<CapabilityContract, RpcError> {
            let mut contract = ExampleHandler.contract()?;
            contract.capability.id = "example.async.secondary".into();
            Ok(contract)
        }

        fn dispatch<'a>(&'a self, operation: &'a str, _: Value) -> AsyncRpcFuture<'a> {
            Box::pin(async move {
                if operation == "value" {
                    Ok(serde_json::json!({"Ok": 11}))
                } else {
                    Err(RpcError::UnknownOperation(operation.into()))
                }
            })
        }
    }

    #[test]
    fn async_router_validates_routes_before_hosting() {
        let first = ExampleHandler.contract().unwrap();
        let second = SecondHandler.contract().unwrap();
        let provider = ProviderDefinition {
            id: "example.async.group".into(),
            capabilities: vec![first.capability.clone(), second.capability.clone()],
        };
        let incomplete = crate::AsyncRpcRouter::new(provider.clone())
            .unwrap()
            .route(ExampleHandler)
            .unwrap();
        assert!(incomplete.validate_routes().is_err());
        assert!(incomplete.route(ExampleHandler).is_err());

        let unexpected = ProviderDefinition {
            id: "example.async.single".into(),
            capabilities: vec![first.capability.clone()],
        };
        assert!(crate::AsyncRpcRouter::new(unexpected)
            .unwrap()
            .route(SecondHandler)
            .is_err());

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = AsyncTcpListener::bind("127.0.0.1:0").await.unwrap();
            let stopping = Arc::new(AtomicBool::new(false));
            // An incomplete route set cannot enter the accept loop.
            let missing = crate::AsyncRpcRouter::new(provider.clone())
                .unwrap()
                .route(ExampleHandler)
                .unwrap();
            assert!(serve_async_tcp_rpc_host(
                listener,
                Arc::new(missing),
                Arc::clone(&stopping),
            )
            .await
            .is_err());
        });
    }

    #[test]
    fn async_router_serves_two_capabilities_over_one_provider_endpoint() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let first = ExampleHandler.contract().unwrap();
            let second = SecondHandler.contract().unwrap();
            let provider = ProviderDefinition {
                id: "example.async.group".into(),
                capabilities: vec![first.capability.clone(), second.capability.clone()],
            };
            let router = crate::AsyncRpcRouter::new(provider.clone())
                .unwrap()
                .route(ExampleHandler)
                .unwrap()
                .route(SecondHandler)
                .unwrap();
            router.validate_routes().unwrap();

            let listener = AsyncTcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let stopping = Arc::new(AtomicBool::new(false));
            let host = tokio::spawn(serve_async_tcp_rpc_host(
                listener,
                Arc::new(router),
                Arc::clone(&stopping),
            ));
            let client = AsyncTcpRpcTransport::new(address);
            for contract in [&first, &second] {
                client.verify(contract).await.unwrap();
                assert_eq!(client.provider(&contract.capability).await.unwrap(), provider);
            }
            assert_eq!(
                client
                    .call(&first.capability, "value", serde_json::json!({}))
                    .await
                    .unwrap(),
                serde_json::json!({"Ok": 7})
            );
            assert_eq!(
                client
                    .call(&second.capability, "value", serde_json::json!({}))
                    .await
                    .unwrap(),
                serde_json::json!({"Ok": 11})
            );
            let invalid = CapabilityRef {
                id: "example.unknown".into(),
                version: 1,
            };
            assert!(matches!(
                client.call(&invalid, "value", serde_json::json!({})).await,
                Err(RpcError::WrongCapability { .. })
            ));
            stopping.store(true, Ordering::Release);
            timeout(Duration::from_secs(2), host)
                .await
                .expect("async router did not shut down")
                .unwrap()
                .unwrap();
        });
    }

}
