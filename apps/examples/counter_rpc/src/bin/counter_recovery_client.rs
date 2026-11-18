//! Verify the SDK automatically reconstructs a complete session after a
//! managed provider dies. Old capability handles are never reused.
use riesel::Application;
use riesel_counter::CounterContract;
use std::io;
use std::time::Duration;

fn main() -> riesel::Result<()> {
    let mut executions = 0;
    let mut previous: Option<std::sync::Arc<dyn riesel_counter::Counter + Send + Sync>> = None;
    Application::new("RecoveryProbe")
        .rpc::<CounterContract>("counter")?
        .run_from_env(move |ctx| {
            executions += 1;
            let counter = ctx.capability::<CounterContract>("counter")?;
            assert_eq!(counter.value()?, 0);
            if executions == 1 {
                previous = Some(counter.clone());
                if !ctx
                    .cancellation_token()
                    .wait_cancelled_timeout(Duration::from_secs(10))
                {
                    return Err(io::Error::other("crashing provider did not cancel session").into());
                }
            } else if executions == 2 {
                let old = previous.take().expect("old session handle exists");
                assert!(matches!(
                    old.value(),
                    Err(riesel_counter::CounterError::Transport(message))
                        if message.contains("expired application session")
                ));
                assert_eq!(counter.add(7)?, 7);
                assert_eq!(counter.label("count")?, "count: 7");
                println!("RIESEL_RECOVERY_OK");
            } else {
                return Err(io::Error::other("unexpected extra application restart").into());
            }
            Ok(())
        })
}
