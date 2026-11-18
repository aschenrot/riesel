//! Fault-injection fixture: first managed process exits unexpectedly after
//! becoming ready; the replacement instance remains alive. The marker file
//! makes this behavior deterministic across separate process executions.
use std::fs::OpenOptions;
use std::io;
use std::thread;
use std::time::Duration;

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let marker = args.next().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "expected shared marker path")
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected exactly one marker path",
        ));
    }
    let first = match OpenOptions::new().write(true).create_new(true).open(marker) {
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => false,
        Err(error) => return Err(error),
    };

    if first {
        thread::spawn(|| {
            thread::sleep(Duration::from_secs(2));
            std::process::exit(67);
        });
    }
    riesel_counter_component::run_counter_server("127.0.0.1:0")
}
