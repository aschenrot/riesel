//! Independent headless GUI stand-in using generated typed Rust capability
//! bindings. No credentials are supplied: successful mutation here also
//! demonstrates that the current loopback transport is NOT authenticated.
use riesel_counter::{AsyncCounterContract, AsyncCounterObserverContract};
use riesel_protocol::{AsyncRpcClientBinding, AsyncTcpRpcTransport, RpcError};
use std::{env, error::Error, net::SocketAddr, sync::Arc};

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

        let public = AsyncCounterContract::bind_async(transport);
        match mode.as_str() {
            "first" => {
                assert_eq!(public.value().await?, 0);
                assert_eq!(public.add(3).await?, 3);
            }
            "second" => {
                // First client exited; backend state remains authoritative.
                assert_eq!(public.value().await?, 3);
                assert_eq!(public.add(2).await?, 5);
            }
            _ => return Err("unknown test mode".into()),
        }
        Ok::<(), Box<dyn Error + Send + Sync>>(())
    })?;
    println!("RESEARCH_FACADE_CLIENT_OK {mode}");
    Ok(())
}
