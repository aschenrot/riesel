#![cfg(feature = "async-tcp")]
//! Real managed-service regression: aborting an in-flight RPC must never
//! detach its already-started provider-owned blocking OS thread.

use riesel_counter::AsyncCounterContract;
use riesel_protocol::{RpcTransport, TcpRpcTransport};
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
fn managed_async_host_joins_abandoned_blocking_worker_on_shutdown() {
    let prefix =
        std::env::temp_dir().join(format!("riesel-provider-worker-{}", std::process::id()));
    let started = prefix.with_extension("started");
    let joined = prefix.with_extension("joined");
    let _ = std::fs::remove_file(&started);
    let _ = std::fs::remove_file(&joined);
    struct MarkerCleanup(std::path::PathBuf, std::path::PathBuf);
    impl Drop for MarkerCleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let _ = std::fs::remove_file(&self.1);
        }
    }
    let _markers = MarkerCleanup(started.clone(), joined.clone());

    let mut child = OwnedChild(
        Command::new(env!("CARGO_BIN_EXE_async_blocking_shutdown_fixture"))
            .env("RIESEL_BIND_ADDRESS", "127.0.0.1:0")
            .env("RIESEL_CONTROL_STDIN", "1")
            .env("RIESEL_TEST_BLOCKING_MARKER", &prefix)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to spawn provider blocking fixture"),
    );

    let stdout = child.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        let mut line = String::new();
        let ready = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = tx.send(ready);
    });
    let line = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("managed host did not become ready")
        .expect("managed host stdout was unreadable");
    reader.join().unwrap();
    let announcement: serde_json::Value = serde_json::from_str(
        line.strip_prefix("RIESEL_READY ")
            .expect("missing managed host readiness"),
    )
    .unwrap();
    let address: SocketAddr = announcement["address"].as_str().unwrap().parse().unwrap();
    assert_eq!(announcement["shutdown"], "stdin_line");

    let capability = AsyncCounterContract::definition().unwrap().capability;
    let request = thread::spawn(move || {
        TcpRpcTransport::new(address).call(&capability, "add", serde_json::json!({"amount": 1}))
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !started.exists() {
        assert!(Instant::now() < deadline, "blocking worker was not started");
        thread::sleep(Duration::from_millis(10));
    }
    // Shutdown aborts the pending RPC future while the OS worker runs.
    // Only a real provider-owned join can observe its cancellation marker
    // before this managed service has finished its own shutdown.
    child
        .0
        .stdin
        .take()
        .unwrap()
        .write_all(b"RIESEL_SHUTDOWN\n")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(exit) = child.0.try_wait().unwrap() {
            assert!(exit.success(), "managed provider did not stop cleanly");
            break;
        }
        assert!(Instant::now() < deadline, "managed host shutdown timed out");
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        joined.exists(),
        "host exited before joining its blocking worker"
    );
    let _ = request.join().unwrap();
}
