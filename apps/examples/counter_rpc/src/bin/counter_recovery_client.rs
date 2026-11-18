//! Verify the SDK automatically reconstructs a complete session after a
//! managed provider dies. Old capability handles are never reused.
use riesel::Application;
use riesel_counter::CounterContract;
use std::io;
use std::time::Duration;

fn main() -> riesel::Result<()> {
    let mut executions = 0;
    Application::new("RecoveryProbe")
        .rpc::<CounterContract>("counter")?
        .run_from_env(move |ctx| {
            executions += 1;
            let counter = ctx.capability::<CounterContract>("counter")?;
            assert_eq!(counter.value()?, 0);
            if executions == 1 {
                if !ctx
                    .cancellation_token()
                    .wait_cancelled_timeout(Duration::from_secs(10))
                {
                    return Err(io::Error::other("crashing provider did not cancel session").into());
                }
            } else if executions == 2 {
                assert_eq!(counter.add(7)?, 7);
                assert_eq!(counter.label("count")?, "count: 7");
                println!("RIESEL_RECOVERY_OK");
            } else {
                return Err(io::Error::other("unexpected extra application restart").into());
            }
            Ok(())
        })
}
