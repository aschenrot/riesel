//! Test-only owned service for real managed-process graph lifecycle checks.
//! This wire fixture is deliberately NOT a Riesel capability RPC protocol.
use std::io::{self, BufRead, Write};
use std::net::TcpListener;

fn main() -> io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    println!(
        "RIESEL_READY {{\"protocol\":1,\"address\":\"{}\",\"shutdown\":\"stdin_line\"}}",
        listener.local_addr()?
    );
    io::stdout().flush()?;

    // The actual Manager-owned graceful-stop channel remains active unless
    // the test explicitly asks this process to crash.
    std::thread::spawn(|| {
        let mut line = String::new();
        if io::stdin().read_line(&mut line).is_ok() && line == "RIESEL_SHUTDOWN\n" {
            std::process::exit(0);
        }
    });

    for connection in listener.incoming() {
        let mut connection = connection?;
        let mut request = String::new();
        if io::BufReader::new(&mut connection).read_line(&mut request)? == 0 {
            continue;
        }
        match request.as_str() {
            "COMMIT\n" => connection.write_all(b"ACK\n")?,
            // Crash without sending a reply after the caller dispatched a
            // mutation. The parent must not infer that it never executed.
            "CRASH\n" => std::process::exit(41),
            _ => connection.write_all(b"UNKNOWN\n")?,
        }
    }
    Ok(())
}
