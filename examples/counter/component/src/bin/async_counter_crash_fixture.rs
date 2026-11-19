//! Fault injection: the first managed native async provider exits after
//! becoming ready; subsequent starts reuse the same marker and stay healthy.
use riesel_counter_component::memory_async_counter_provider;
use std::fs::OpenOptions;
use std::io;
use std::thread;
use std::time::Duration;

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let marker = args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "expected recovery marker path")
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected exactly one recovery marker path",
        ));
    }
    let first = match OpenOptions::new().write(true).create_new(true).open(marker) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error),
    };
    if first {
        thread::spawn(|| {
            thread::sleep(Duration::from_millis(1_250));
            std::process::exit(73);
        });
    }
    // The first CLI argument belongs to this fault-injection fixture,
    // not to the shared component host's optional bind-address parser.
    // Managed processes always receive their OS-assigned bind request in
    // RIESEL_BIND_ADDRESS, so the normal host still owns readiness/shutdown.
    let address = std::env::var("RIESEL_BIND_ADDRESS")
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    memory_async_counter_provider()
        .map_err(io::Error::other)?
        .serve(&address)
}
