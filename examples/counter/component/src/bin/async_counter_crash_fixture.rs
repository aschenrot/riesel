//! Fault injection: the first managed native async provider exits after
//! becoming ready; subsequent starts reuse the same marker and stay healthy.
use riesel_counter_component::memory_async_counter_provider;
use riesel_protocol::run_component_from_env;
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
    run_component_from_env(|address| {
        memory_async_counter_provider()
            .map_err(io::Error::other)?
            .serve(address)
    })
}
