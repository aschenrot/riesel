//! Counter example: separate capability contract and provider component,
//! generated RPC and both local/managed Riesel execution paths.
pub use riesel_counter::{
    Counter, CounterContract, CounterError, CounterRpcClient, CounterRpcDispatcher,
};
pub use riesel_counter_component::MemoryCounter;

/// Shared example work for local and runtime-managed deployments.
pub fn run_demo(ctx: &riesel::Context<'_>) -> Result<(), riesel::ConnectorError> {
    let counter = ctx.capability::<CounterContract>("counter")?;
    assert_eq!(counter.value()?, 0);
    assert_eq!(counter.add(3)?, 3);
    assert_eq!(counter.label("count")?, "count: 3");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_protocol::{
        CallError, LocalRpc, RpcError, RpcHandler, RpcTransport, TcpRpcTransport,
        handle_tcp_connection,
    };
    use serde_json::json;
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn two_local_capabilities_share_one_component_state() {
        use riesel::Application;
        use riesel_counter::{CounterObserverContract, CounterObserver};
        use riesel_counter_component::memory_counter_provider;

        Application::new("DualLocal")
            .rpc::<CounterContract>("counter")
            .unwrap()
            .rpc::<CounterObserverContract>("observer")
            .unwrap()
            .provider(memory_counter_provider().unwrap())
            .unwrap()
            .run(|ctx| {
                let writer = ctx.capability::<CounterContract>("counter")?;
                let reader = ctx.capability::<CounterObserverContract>("observer")?;
                assert_eq!(reader.observe()?, 0);
                assert_eq!(writer.add(13)?, 13);
                assert_eq!(reader.observe()?, 13);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn typed_sdk_provider_shortcut_requires_no_factory_implementation() {
        use riesel::Application;
        use std::sync::Arc;

        Application::new("Counter")
            .rpc::<CounterContract>("counter")
            .unwrap()
            .provide::<CounterContract, _>("riesel.counter.memory", || {
                Arc::new(MemoryCounter::default()) as Arc<dyn Counter + Send + Sync>
            })
            .unwrap()
            .run(run_demo)
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
}
