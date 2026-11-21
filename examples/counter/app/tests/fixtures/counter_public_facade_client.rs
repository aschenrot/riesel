//! Independent headless GUI stand-in using generated typed Rust capability
//! bindings. No credentials are supplied: successful mutation here also
//! demonstrates that the current loopback transport is NOT authenticated.
use riesel_counter::{AsyncCounterContract, AsyncCounterObserverContract, CounterError};
use riesel_protocol::{
    AsyncRpcClientBinding, AsyncRpcTransport, AsyncTcpRpcTransport, CallError, RpcError,
};
use std::{
    env,
    error::Error,
    io::{self, Write},
    net::SocketAddr,
    sync::Arc,
};

fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let mut args = env::args().skip(1);
    let address: SocketAddr = args.next().ok_or("missing address")?.parse()?;
    let mode = args.next().ok_or("missing test mode")?;
    if args.next().is_some() || !address.ip().is_loopback() {
        return Err("expected loopback address and exactly one test mode".into());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let transport = Arc::new(AsyncTcpRpcTransport::new(address));
        transport
            .verify(&AsyncCounterContract::definition()?)
            .await?;

        // Internal Observer is not reachable on this explicitly selected
        // interface endpoint. Contract metadata is not authentication.
        let internal = AsyncCounterObserverContract::definition()?;
        assert!(matches!(
            transport.describe(&internal.capability).await,
            Err(RpcError::WrongCapability { .. })
        ));

        // Both generated and manually forwarded endpoints must reject
        // malformed input and preserve domain vs transport failures.
        let expected = AsyncCounterContract::definition()?;
        assert!(matches!(
            transport
                .call(
                    &expected.capability,
                    "add",
                    serde_json::json!({"amount": 1, "extra": true}),
                )
                .await,
            Err(RpcError::InvalidArguments(_))
        ));
        let public = AsyncCounterContract::bind_async(transport);
        assert!(matches!(
            public.add(0).await,
            Err(CallError::Domain(CounterError::ZeroIncrement))
        ));
        match mode.as_str() {
            "first" => {
                assert_eq!(public.value().await?, 0);
                assert_eq!(public.add(3).await?, 3);
                assert_eq!(public.label("count").await?, "count: 3");
            }
            "second" => {
                // First client exited; backend state remains authoritative.
                assert_eq!(public.value().await?, 3);
                assert_eq!(public.add(2).await?, 5);
                assert_eq!(public.label("count").await?, "count: 5");
            }
            "lost-reply" => {
                // A remote handler applies add(3) but intentionally never
                // returns its response. A transport timeout does not prove
                // that a mutation failed, and must not trigger retry.
                assert_eq!(public.value().await?, 0);
                assert!(matches!(
                    public.add(3).await,
                    Err(CallError::Rpc(RpcError::Transport(_)))
                ));
                assert_eq!(public.value().await?, 3);
                assert_eq!(public.add(2).await?, 5);
            }
            "stale" => {
                // Deliberate NEGATIVE research: keep this same generated
                // client/binding alive while the entire app-host OS process
                // exits and a fresh app session binds the same endpoint.
                // No new verification, descriptor or session attachment is
                // constructed after the restart.
                assert_eq!(public.value().await?, 0);
                assert_eq!(public.add(5).await?, 5);
                println!("RESEARCH_FACADE_STALE_PHASE1_READY");
                io::stdout().flush()?;
                let mut instruction = String::new();
                io::stdin().read_line(&mut instruction)?;
                if instruction.trim() != "CONTINUE" {
                    return Err("stale-client fixture expected CONTINUE".into());
                }
                // Fresh backend state is 0; an externally fenced client
                // should reject this OLD binding before mutating it.
                // The current unauthenticated reference transport instead
                // opens a new connection to the reused address.
                assert_eq!(public.value().await?, 0);
                assert_eq!(public.add(2).await?, 2);
                assert_eq!(public.add(3).await?, 5);
                println!("RESEARCH_FACADE_STALE_REBIND_ACCEPTED");
                io::stdout().flush()?;
            }
            _ => return Err("unknown test mode".into()),
        }
        Ok::<(), Box<dyn Error + Send + Sync>>(())
    })?;
    println!("RESEARCH_FACADE_CLIENT_OK {mode}");
    Ok(())
}
