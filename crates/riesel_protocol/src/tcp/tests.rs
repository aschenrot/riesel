use super::*;

struct ParallelHandler {
    slow_started: Arc<AtomicBool>,
}

impl RpcHandler for ParallelHandler {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        use riesel_schema::{OperationDefinition, OperationKind, TypeRef};
        let method = || OperationDefinition {
            kind: OperationKind::Query,
            description: "Return a value".into(),
            inputs: vec![],
            output: TypeRef::U64,
            error: TypeRef::Unit,
        };
        Ok(CapabilityContract {
            capability: CapabilityRef {
                id: "example.concurrent".into(),
                version: 1,
            },
            operations: [("slow".into(), method()), ("fast".into(), method())]
                .into_iter()
                .collect(),
            types: Default::default(),
        })
    }

    fn dispatch(&self, operation: &str, _: Value) -> Result<Value, RpcError> {
        if operation == "slow" {
            self.slow_started.store(true, Ordering::Release);
            thread::sleep(Duration::from_millis(1_000));
        }
        Ok(serde_json::json!(7))
    }
}

#[test]
fn slow_connections_do_not_block_independent_requests() {
    use std::time::Instant;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopping = Arc::new(AtomicBool::new(false));
    let slow_started = Arc::new(AtomicBool::new(false));
    let host = {
        let stopping = Arc::clone(&stopping);
        let slow_started = Arc::clone(&slow_started);
        thread::spawn(move || {
            serve_connections(listener, &ParallelHandler { slow_started }, &stopping, true).unwrap()
        })
    };
    let capability = CapabilityRef {
        id: "example.concurrent".into(),
        version: 1,
    };
    let slow_call = {
        let capability = capability.clone();
        thread::spawn(move || {
            TcpRpcTransport::new(address).call(&capability, "slow", serde_json::json!({}))
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while !slow_started.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline, "slow RPC did not start");
        thread::sleep(Duration::from_millis(5));
    }
    let now = Instant::now();
    let fast = TcpRpcTransport::new(address)
        .call(&capability, "fast", serde_json::json!({}))
        .unwrap();
    let elapsed = now.elapsed();
    stopping.store(true, Ordering::Release);
    assert_eq!(fast, serde_json::json!(7));
    assert!(
        elapsed < Duration::from_millis(700),
        "fast RPC waited for unrelated slow request: {elapsed:?}"
    );
    assert_eq!(slow_call.join().unwrap().unwrap(), serde_json::json!(7));
    host.join().unwrap();
}

#[test]
fn rejects_non_loopback_plaintext_clients() {
    let client = TcpRpcTransport::new("8.8.8.8:443".parse().unwrap());
    let capability = CapabilityRef {
        id: "example.counter".into(),
        version: 1,
    };
    assert!(matches!(
        client.call(&capability, "value", serde_json::json!({})),
        Err(RpcError::Transport(_))
    ));
}

#[test]
fn application_transport_reuses_a_connection_for_sequential_calls() {
    use std::sync::atomic::AtomicUsize;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let server = {
        let accepted = Arc::clone(&accepted);
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            accepted.fetch_add(1, Ordering::SeqCst);
            let handler = ParallelHandler {
                slow_started: Arc::new(AtomicBool::new(false)),
            };
            handle_tcp_connection(&handler, &mut stream).unwrap();
            handle_tcp_connection(&handler, &mut stream).unwrap();
        })
    };
    let transport = TcpRpcTransport::with_session(address, SessionToken::new());
    let capability = CapabilityRef {
        id: "example.concurrent".into(),
        version: 1,
    };
    for _ in 0..2 {
        assert_eq!(
            transport
                .call(&capability, "fast", serde_json::json!({}))
                .unwrap(),
            serde_json::json!(7)
        );
    }
    server.join().unwrap();
    assert_eq!(accepted.load(Ordering::SeqCst), 1);
}
