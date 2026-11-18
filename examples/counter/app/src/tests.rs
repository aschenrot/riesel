use super::*;
use riesel_protocol::{
    CallError, LocalRpc, RpcError, RpcHandler, RpcTransport, TcpRpcTransport, handle_tcp_connection,
};
use serde_json::json;
use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn two_local_capabilities_share_one_component_state() {
    use riesel::{Application, Capability};
    use riesel_counter::{CounterObserver, CounterObserverContract};
    use riesel_counter_component::memory_counter_provider;

    Application::new("DualLocal")
        .handler_with_aliases(
            |writer: Capability<CounterContract>,
             reader: Capability<CounterObserverContract>|
             -> Result<(), CounterError> {
                assert_eq!(reader.observe()?, 0);
                assert_eq!(writer.add(13)?, 13);
                assert_eq!(reader.observe()?, 13);
                Ok(())
            },
            ["counter", "observer"],
        )
        .unwrap()
        .provider(memory_counter_provider().unwrap())
        .unwrap()
        .run()
        .unwrap();
}

#[test]
fn typed_sdk_provider_shortcut_requires_no_factory_implementation() {
    use riesel::Application;
    use std::sync::Arc;

    Application::new("Counter")
        .handler_with_aliases(run_demo, ["counter"])
        .unwrap()
        .provide::<CounterContract, _>("riesel.counter.memory", || {
            Arc::new(MemoryCounter::default()) as Arc<dyn Counter + Send + Sync>
        })
        .unwrap()
        .run()
        .unwrap();
}

#[test]
fn generated_local_capability_handles_expire_with_their_session() {
    use riesel::Application;
    use riesel_counter_component::memory_counter_provider;
    use std::sync::{Arc, Mutex};

    let captured = Arc::new(Mutex::new(None));
    let for_callback = Arc::clone(&captured);
    Application::new("ScopedCounter")
        .rpc::<CounterContract>("counter")
        .unwrap()
        .provider(memory_counter_provider().unwrap())
        .unwrap()
        .run(move |ctx| {
            let counter = ctx.capability::<CounterContract>("counter")?.clone();
            assert_eq!(counter.add(2)?, 2);
            *for_callback.lock().unwrap() = Some(counter);
            Ok(())
        })
        .unwrap();

    let old: Arc<dyn Counter + Send + Sync> = captured.lock().unwrap().take().unwrap();
    assert!(matches!(
        old.value(),
        Err(CounterError::Transport(message)) if message.contains("expired application session")
    ));
}

#[test]
fn managed_component_router_rejects_missing_and_duplicate_capability_handlers() {
    use riesel_protocol::RpcRouter;
    let router =
        RpcRouter::new(riesel_counter_component::memory_provider_definition().unwrap()).unwrap();
    assert!(matches!(
        router.validate_routes(),
        Err(RpcError::Contract(_))
    ));

    let router = router
        .route(CounterRpcDispatcher::new(MemoryCounter::default()))
        .unwrap();
    assert!(matches!(
        router.validate_routes(),
        Err(RpcError::Contract(_))
    ));
    assert!(matches!(
        router.route(CounterRpcDispatcher::new(MemoryCounter::default())),
        Err(RpcError::Contract(_))
    ));
}

#[test]
fn generated_typed_binding_supports_non_tcp_rpc_transport() {
    use riesel_protocol::{RpcClientBinding, RpcTransport};
    use std::sync::Arc;

    let transport: Arc<dyn RpcTransport> = Arc::new(LocalRpc::new(CounterRpcDispatcher::new(
        MemoryCounter::default(),
    )));
    let client = CounterContract::bind(transport);
    assert_eq!(client.add(19).unwrap(), 19);
    assert_eq!(client.value().unwrap(), 19);
}

#[test]
fn generated_client_and_dispatcher_support_commands_and_queries() {
    let handler = CounterRpcDispatcher::new(MemoryCounter::default());
    let client = CounterRpcClient::new(LocalRpc::new(handler));

    assert_eq!(client.value().unwrap(), 0);
    assert_eq!(client.add(3).unwrap(), 3);
    assert_eq!(client.label("count").unwrap(), "count: 3");
    assert!(matches!(
        client.add(0),
        Err(CallError::Domain(CounterError::ZeroIncrement))
    ));
    assert_eq!(client.value().unwrap(), 3);
}

#[test]
fn generated_dispatcher_validates_operations_and_argument_shapes() {
    let handler = CounterRpcDispatcher::new(MemoryCounter::default());
    assert_eq!(
        handler.contract().unwrap().capability,
        CounterContract::definition().unwrap().capability
    );
    assert!(matches!(
        handler.dispatch("unknown", json!({})),
        Err(RpcError::UnknownOperation(_))
    ));
    assert!(matches!(
        handler.dispatch("add", json!({"amount": "not a number"})),
        Err(RpcError::InvalidArguments(_))
    ));
    assert!(matches!(
        handler.dispatch("value", json!({"extra": true})),
        Err(RpcError::InvalidArguments(_))
    ));
}

#[test]
fn local_transport_rejects_a_different_capability_or_version() {
    let transport = LocalRpc::new(CounterRpcDispatcher::new(MemoryCounter::default()));
    let mut capability = CounterContract::definition().unwrap().capability;
    capability.version += 1;
    assert!(matches!(
        transport.call(&capability, "value", json!({})),
        Err(RpcError::WrongCapability { .. })
    ));
}

#[test]
fn domain_overflow_is_not_a_transport_error() {
    let handler = CounterRpcDispatcher::new(MemoryCounter::default());
    let client = CounterRpcClient::new(LocalRpc::new(handler));
    assert_eq!(client.add(u64::MAX).unwrap(), u64::MAX);
    assert!(matches!(
        client.add(1),
        Err(CallError::Domain(CounterError::Overflow))
    ));
}
#[test]
fn generated_client_and_dispatcher_work_over_real_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();

    let server = thread::spawn(move || {
        let handler = CounterRpcDispatcher::new(MemoryCounter::default());
        let deadline = Instant::now() + Duration::from_secs(8);
        let mut handled = 0;
        while handled < 6 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    handle_tcp_connection(&handler, &mut stream).unwrap();
                    handled += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(
                        Instant::now() < deadline,
                        "timed out waiting for RPC requests"
                    );
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("could not accept RPC connection: {error}"),
            }
        }
    });

    let transport = TcpRpcTransport::new(address);
    let contract = CounterContract::definition().unwrap();
    transport.verify(&contract).unwrap();
    assert!(matches!(
        transport.call(&contract.capability, "not_an_operation", json!({})),
        Err(RpcError::UnknownOperation(_))
    ));

    let client = CounterRpcClient::new(transport);
    assert_eq!(client.value().unwrap(), 0);
    assert_eq!(client.add(7).unwrap(), 7);
    assert_eq!(client.label("value").unwrap(), "value: 7");
    assert!(matches!(
        client.add(0),
        Err(CallError::Domain(CounterError::ZeroIncrement))
    ));
    server.join().unwrap();
}

#[test]
fn handler_parameters_share_explicit_contract_metadata_without_name_based_aliases() {
    use riesel::{Application, Capability};
    use riesel_counter::CounterObserverContract;

    let prepared = Application::new("Parity")
        .handler_with_aliases(
            |_counter: Capability<CounterContract>| -> Result<(), CounterError> { Ok(()) },
            ["counter"],
        )
        .unwrap();
    let existing = Application::new("Parity")
        .rpc::<CounterContract>("counter")
        .unwrap();
    assert_eq!(
        prepared.definition_json().unwrap(),
        existing.definition_json().unwrap()
    );

    let implicit = Application::new("Implicit")
        .handler(
            |_renamed_variable: Capability<CounterContract>| -> Result<(), CounterError> { Ok(()) },
        )
        .unwrap();
    assert_eq!(
        implicit.definition().requires[0].name,
        CounterContract::definition()
            .unwrap()
            .capability
            .default_requirement_alias()
    );

    let distinct_instances = Application::new("TwoCounters")
        .handler_with_aliases(
            |_first: Capability<CounterContract>,
             _second: Capability<CounterContract>|
             -> Result<(), CounterError> { Ok(()) },
            ["first", "second"],
        )
        .unwrap();
    assert_eq!(distinct_instances.definition().requires.len(), 2);

    let conflict = Application::new("Conflict").handler_with_aliases(
        |_writer: Capability<CounterContract>,
         _reader: Capability<CounterObserverContract>|
         -> Result<(), CounterError> { Ok(()) },
        ["shared", "shared"],
    );
    assert!(matches!(
        conflict,
        Err(riesel::SdkError::RequirementConflict(name)) if name == "shared"
    ));
}

#[test]
fn generated_async_client_preserves_domain_and_transport_errors() {
    use riesel_protocol::{AsyncRpcTransport, CallError, RpcError};
    use riesel_schema::CapabilityRef;
    use serde_json::Value;
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    struct ReadyTransport(CounterRpcDispatcher<MemoryCounter>);

    impl AsyncRpcTransport for ReadyTransport {
        fn call<'a>(
            &'a self,
            capability: &'a CapabilityRef,
            operation: &'a str,
            arguments: Value,
        ) -> Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'a>> {
            // A ready, in-memory test double; real network transport must
            // use nonblocking I/O rather than this synchronous dispatcher.
            Box::pin(async move { self.0.dispatch_for(capability, operation, arguments) })
        }
    }

    fn run_ready<F: Future>(future: F) -> F::Output {
        let mut pinned = std::pin::pin!(future);
        match pinned
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("ready test transport unexpectedly blocked"),
        }
    }

    let client = riesel_counter::CounterAsyncClient::new(ReadyTransport(
        CounterRpcDispatcher::new(MemoryCounter::default()),
    ));
    assert_eq!(run_ready(client.value()).unwrap(), 0);
    assert_eq!(run_ready(client.add(4)).unwrap(), 4);
    assert!(matches!(
        run_ready(client.add(0)),
        Err(CallError::Domain(CounterError::ZeroIncrement))
    ));
    assert_eq!(run_ready(client.label("v")).unwrap(), "v: 4");
}

#[test]
fn generated_async_client_operates_over_real_nonblocking_tcp() {
    use riesel_protocol::{AsyncTcpRpcTransport, RpcError, SessionToken};
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let handler = CounterRpcDispatcher::new(MemoryCounter::default());
        for _ in 0..5 {
            let (mut connection, _) = listener.accept().unwrap();
            handle_tcp_connection(&handler, &mut connection).unwrap();
        }
    });

    let session = SessionToken::new();
    let transport = AsyncTcpRpcTransport::with_session(address, session.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        transport.verify(&CounterContract::definition().unwrap()).await.unwrap();
        let client = riesel_counter::CounterAsyncClient::new(transport);
        assert_eq!(client.value().await.unwrap(), 0);
        assert_eq!(client.add(8).await.unwrap(), 8);
        assert!(matches!(
            client.add(0).await,
            Err(CallError::Domain(CounterError::ZeroIncrement))
        ));
        assert_eq!(client.label("count").await.unwrap(), "count: 8");
        session.invalidate();
        assert!(matches!(
            client.value().await,
            Err(CallError::Rpc(RpcError::SessionExpired))
        ));
    });
    server.join().unwrap();
}
