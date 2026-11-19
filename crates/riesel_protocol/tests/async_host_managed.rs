#![cfg(feature = "async-tcp")]
//! Exercise the exact stdout readiness and stdin shutdown contract used by
//! the existing managed application runtime, using a real child process.

use riesel_protocol::{RpcTransport, TcpRpcTransport};
use riesel_schema::CapabilityRef;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn managed_async_host_shares_state_and_stops_on_private_control_request() {
    let mut child = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_riesel_async_host_fixture"))
            .env("RIESEL_BIND_ADDRESS", "127.0.0.1:0")
            .env("RIESEL_CONTROL_STDIN", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("could not launch async managed-host fixture"),
    );

    // A broken host must not hang the test indefinitely while waiting for
    // its readiness frame. OwnedChild guarantees process cleanup on failure.
    let stdout = child.0.stdout.take().unwrap();
    let (sender, receiver) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("async host did not advertise readiness")
        .expect("failed to read async host readiness");
    reader.join().unwrap();
    let payload = line
        .strip_prefix("RIESEL_READY ")
        .expect("missing managed-service readiness prefix");
    let ready: Value = serde_json::from_str(payload).unwrap();
    assert_eq!(ready["protocol"], 1);
    assert_eq!(ready["shutdown"], "stdin_line");
    let address: SocketAddr = ready["address"].as_str().unwrap().parse().unwrap();
    assert!(address.ip().is_loopback());
    assert_ne!(address.port(), 0);

    let writer = CapabilityRef {
        id: "example.async.writer".into(),
        version: 1,
    };
    let observer = CapabilityRef {
        id: "example.async.observer".into(),
        version: 1,
    };
    let transport = TcpRpcTransport::new(address);
    let provider = transport.provider(&writer).unwrap();
    assert_eq!(provider.id, "example.async.managed");
    assert!(provider.capabilities.contains(&writer));
    assert!(provider.capabilities.contains(&observer));
    transport.describe(&writer).unwrap();
    transport.describe(&observer).unwrap();

    assert_eq!(
        transport
            .call(&writer, "add", json!({"amount": 5}))
            .unwrap(),
        json!({"Ok": 5})
    );
    assert_eq!(
        transport.call(&observer, "value", json!({})).unwrap(),
        json!({"Ok": 5})
    );
    assert_eq!(
        transport
            .call(&writer, "add", json!({"amount": 2}))
            .unwrap(),
        json!({"Ok": 7})
    );
    assert_eq!(
        transport.call(&observer, "value", json!({})).unwrap(),
        json!({"Ok": 7})
    );

    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"RIESEL_SHUTDOWN\n")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success(), "async managed host did not shut down cleanly");
            break;
        }
        assert!(Instant::now() < deadline, "async managed host shutdown timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
