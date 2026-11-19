//! Verify the real Riesel/SQLite host across independent processes.
#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

fn start(database: &Path) -> (Child, u16) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_todo_http"))
        .arg("--db")
        .arg(database)
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();

    let stderr = child.stderr.take().unwrap();
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stderr).read_line(&mut line);
        let _ = send.send(line);
    });
    let first_line = match receive.recv_timeout(Duration::from_secs(10)) {
        Ok(line) => line,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Todo HTTP did not report its address: {error}");
        }
    };
    let port = match first_line
        .trim()
        .strip_prefix("Todo HTTP: http://127.0.0.1:")
        .and_then(|port| port.parse::<u16>().ok())
    {
        Some(port) => port,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("unexpected Todo HTTP startup result: {first_line:?}");
        }
    };
    (child, port)
}

fn request(port: u16, method: &str, body: Option<&str>) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let payload = body.unwrap_or("");
    let mut request =
        format!("{method} /api/tasks HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n");
    if body.is_some() {
        request.push_str(&format!(
            "Origin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n",
            payload.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(payload);
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

fn terminate(mut child: Child) {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(status.success(), "failed to send SIGTERM");
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "HTTP process failed on SIGTERM: {status}");
            return;
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("Todo HTTP did not shut down after SIGTERM");
}

#[test]
fn sqlite_state_survives_http_host_restart_and_signals() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("todo-http.db");

    let (first, port) = start(&database);
    let created = request(port, "POST", Some(r#"{"title":"Persisted"}"#));
    assert!(created.starts_with("HTTP/1.1 201 "), "{created}");
    assert!(created.contains("Persisted"), "{created}");
    terminate(first);

    assert!(database.exists(), "database was not retained");
    let (second, port) = start(&database);
    let listed = request(port, "GET", None);
    assert!(listed.starts_with("HTTP/1.1 200 "), "{listed}");
    assert!(listed.contains("Persisted"), "{listed}");
    terminate(second);
}
