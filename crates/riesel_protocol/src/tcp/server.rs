use super::*;

const RPC_WORKERS: usize = 4;
const RPC_PENDING_CONNECTIONS: usize = 32;

/// Run a generated RPC host with the standard managed-service lifecycle.
/// The caller supplies only a typed handler and bind address; the shared
/// transport owns loopback enforcement, ready frames and private shutdown.
pub fn run_tcp_rpc_host<H: RpcHandler>(address: &str, handler: &H) -> io::Result<()> {
    let address: SocketAddr = address.parse().map_err(io::Error::other)?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unauthenticated RPC services must listen on loopback",
        ));
    }
    let listener = TcpListener::bind(address)?;
    let controlled = std::env::var_os("RIESEL_CONTROL_STDIN").is_some();
    let stopping = Arc::new(AtomicBool::new(false));

    if controlled {
        listener.set_nonblocking(true)?;
        let stopping = Arc::clone(&stopping);
        thread::spawn(move || {
            let mut request = String::new();
            if io::stdin().read_line(&mut request).is_ok()
                && request.trim_end() == "RIESEL_SHUTDOWN"
            {
                stopping.store(true, Ordering::Release);
            }
        });
    }

    let ready = serde_json::json!({
        "protocol": 1,
        "address": listener.local_addr()?.to_string(),
        "shutdown": controlled.then_some("stdin_line")
    });
    println!("RIESEL_READY {}", ready);
    io::stdout().flush()?;

    serve_connections(listener, handler, &stopping, controlled)
}

/// Execute a fixed number of request workers. The bounded admission queue
/// prevents one slow socket from serializing every other client or creating
/// an unlimited number of per-connection threads.
pub(super) fn serve_connections<H: RpcHandler>(
    listener: TcpListener,
    handler: &H,
    stopping: &AtomicBool,
    controlled: bool,
) -> io::Result<()> {
    let (sender, receiver) = mpsc::sync_channel::<TcpStream>(RPC_PENDING_CONNECTIONS);
    let receiver = Arc::new(Mutex::new(receiver));

    thread::scope(|scope| {
        for _ in 0..RPC_WORKERS {
            let worker_receiver = Arc::clone(&receiver);
            scope.spawn(move || {
                loop {
                    let connection = worker_receiver
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .recv();
                    let Ok(mut stream) = connection else {
                        break;
                    };
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    // A nonblocking listener may yield a nonblocking stream
                    // on Windows, so normalize it before framed RPC reads.
                    if let Err(error) = stream.set_nonblocking(false) {
                        eprintln!("RPC socket setup failed: {error}");
                    } else if let Err(error) = handle_tcp_connection(handler, &mut stream) {
                        eprintln!("RPC request failed: {error}");
                    }
                }
            });
        }

        let enqueue = |stream| match sender.try_send(stream) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                eprintln!("RPC connection rejected: all workers and queue slots are busy");
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                eprintln!("RPC connection rejected: request workers have stopped");
            }
        };

        let mut result = Ok(());
        if controlled {
            while !stopping.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => enqueue(stream),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
        } else {
            for connection in listener.incoming() {
                match connection {
                    Ok(stream) => enqueue(stream),
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }
        }

        // Workers finish active bounded I/O, but ignore pending requests once
        // the controlled host is shutting down. No new work can be enqueued.
        drop(enqueue);
        drop(sender);
        result
    })
}

/// Handle exactly one framed call on an already accepted TCP connection.
/// Host code owns listener lifetime, concurrency, and process management.
pub fn handle_tcp_connection(
    handler: &impl RpcHandler,
    stream: &mut TcpStream,
) -> Result<(), RpcError> {
    configure_timeouts(stream)?;
    let request: WireRequest = read_json_line(stream)?;
    let result = if request.version != WIRE_VERSION {
        Err(RpcError::Transport(format!(
            "unsupported RPC request protocol version: {}",
            request.version
        )))
    } else {
        let contract = handler.contract()?;
        if request.capability != contract.capability {
            Err(RpcError::WrongCapability {
                expected: format!("{}@{}", contract.capability.id, contract.capability.version),
                received: format!("{}@{}", request.capability.id, request.capability.version),
            })
        } else if request.operation == PROVIDER {
            if request.arguments != serde_json::json!({}) {
                Err(RpcError::InvalidArguments(
                    "provider inspection takes no arguments".into(),
                ))
            } else {
                handler
                    .provider()
                    .ok_or_else(|| RpcError::Contract("service has no registered provider".into()))
                    .and_then(|provider| {
                        serde_json::to_value(provider)
                            .map_err(|error| RpcError::Codec(error.to_string()))
                    })
            }
        } else if request.operation == DESCRIBE {
            if request.arguments != serde_json::json!({}) {
                Err(RpcError::InvalidArguments(
                    "contract inspection takes no arguments".into(),
                ))
            } else {
                serde_json::to_value(contract).map_err(|error| RpcError::Codec(error.to_string()))
            }
        } else {
            handler.dispatch(&request.operation, request.arguments)
        }
    };
    write_json_line(
        stream,
        &WireResponse {
            version: WIRE_VERSION,
            result,
        },
    )
}
