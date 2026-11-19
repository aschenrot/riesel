#![cfg(feature = "http")]
//! Exercise the real HTTP process, not only a synthetic in-memory router.
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

struct Server {
    child: Child,
    authority: String,
}

impl Server {
    fn start(path: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_todo_http"))
            .arg("--db")
            .arg(path)
            .arg("--port")
            .arg("0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(output).read_line(&mut line);
            let _ = tx.send(line);
        });
        let line = match rx.recv_timeout(Duration::from_secs(20)) {
            Ok(line) if line.starts_with("TODO_HTTP_READY ") => line,
            result => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("HTTP server did not announce readiness: {result:?}");
            }
        };
        let authority = line
            .trim()
            .strip_prefix("TODO_HTTP_READY ")
            .unwrap()
            .to_string();
        assert!(authority.starts_with("127.0.0.1:"), "{authority}");
        Self { child, authority }
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: &str,
        host: Option<&str>,
        origin: Option<&str>,
        fetch_site: Option<&str>,
        media_type: Option<&str>,
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(&self.authority).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(8)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(8)))
            .unwrap();
        let mut headers = String::new();
        if let Some(host) = host {
            headers.push_str(&format!("Host: {host}\r\n"));
        }
        if let Some(origin) = origin {
            headers.push_str(&format!("Origin: {origin}\r\n"));
        }
        if let Some(fetch_site) = fetch_site {
            headers.push_str(&format!("Sec-Fetch-Site: {fetch_site}\r\n"));
        }
        if let Some(content_type) = media_type {
            headers.push_str(&format!("Content-Type: {content_type}\r\n"));
        }
        let request = format!(
            "{method} {path} HTTP/1.1\r\n{headers}Connection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        let text = String::from_utf8(response).unwrap();
        let (header, body) = text.split_once("\r\n\r\n").unwrap();
        let status = header.split_whitespace().nth(1).unwrap().parse().unwrap();
        let payload = if header
            .to_ascii_lowercase()
            .contains("transfer-encoding: chunked")
        {
            let mut remaining = body;
            let mut chunks = String::new();
            loop {
                let (line, rest) = remaining.split_once("\r\n").unwrap();
                let size = usize::from_str_radix(line.split(';').next().unwrap(), 16).unwrap();
                if size == 0 {
                    break;
                }
                chunks.push_str(&rest[..size]);
                remaining = &rest[size + 2..];
            }
            chunks
        } else {
            body.to_string()
        };
        (status, payload)
    }

    fn get(&self, path: &str) -> (u16, String) {
        self.request("GET", path, "", Some(&self.authority), None, None, None)
    }

    fn json(&self, method: &str, path: &str, body: &str) -> (u16, String) {
        let origin = format!("http://{}", self.authority);
        self.request(
            method,
            path,
            body,
            Some(&self.authority),
            Some(&origin),
            Some("same-origin"),
            Some("application/json"),
        )
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn local_http_crud_conflicts_paging_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("tasks.db");
    let server = Server::start(&db);
    let (status, body) = server.get("/api/v1/tasks");
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        json!(serde_json::from_str::<Value>(&body).unwrap()["records"]),
        json!([])
    );

    let (status, body) = server.json("POST", "/api/v1/tasks", r#"{"title":"Alpha"}"#);
    assert_eq!(status, 201, "{body}");
    let created: Value = serde_json::from_str(&body).unwrap();
    let id = Uuid::parse_str(created["id"].as_str().unwrap()).unwrap();
    assert_eq!(created["revision"], "1");
    assert_eq!(created["completed"], false);
    let path = format!("/api/v1/tasks/{id}");

    let (status, body) = server.json("PATCH", &path, r#"{"revision":"1","title":"Beta"}"#);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["revision"],
        "2"
    );

    let (status, body) = server.json("PATCH", &path, r#"{"revision":"1","completed":true}"#);
    assert_eq!(status, 409, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["error"]["code"],
        "revision_conflict"
    );

    let (status, body) = server.json("PATCH", &path, r#"{"revision":"2","completed":true}"#);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["completed"],
        true
    );

    let (status, body) = server.get("/api/v1/tasks?limit=1");
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["records"][0]["revision"],
        "3"
    );
    assert_eq!(server.get("/api/v1/tasks?limit=0").0, 400);
    assert_eq!(server.get("/api/v1/tasks/not-a-uuid").0, 400);
    assert_eq!(server.get(&format!("/api/v1/tasks/{}", Uuid::nil())).0, 404);
    drop(server);

    let server = Server::start(&db);
    let (status, body) = server.get(&path);
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["title"],
        "Beta"
    );
    assert_eq!(server.json("DELETE", &path, r#"{"revision":"1"}"#).0, 409);
    assert_eq!(server.json("DELETE", &path, r#"{"revision":"3"}"#).0, 204);
    assert_eq!(server.get(&path).0, 404);
}

#[test]
fn security_guards_precede_mutations_and_invalid_requests_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("tasks.db");
    let server = Server::start(&db);
    let valid_origin = format!("http://{}", server.authority);

    let foreign_host = server.request(
        "GET",
        "/api/v1/tasks",
        "",
        Some("evil.example:8080"),
        None,
        None,
        None,
    );
    assert_eq!(foreign_host.0, 403);
    // An untrusted website must not read data via Fetch Metadata either.
    assert_eq!(
        server.request(
            "GET", "/api/v1/tasks", "", Some(&server.authority),
            None, Some("cross-site"), None
        ).0,
        403
    );
    assert_eq!(
        server.request(
            "GET", "/api/v1/tasks", "", Some(&server.authority),
            None, Some("same-site"), None
        ).0,
        403
    );
    assert_eq!(
        server.request(
            "POST", "/api/v1/tasks", r#"{"title":"forged"}"#,
            Some(&server.authority), Some("null"), None, Some("application/json")
        ).0,
        403
    );
    let foreign_post = server.request(
        "POST",
        "/api/v1/tasks",
        r#"{"title":"forged"}"#,
        Some(&server.authority),
        Some("https://evil.example"),
        Some("cross-site"),
        Some("application/json"),
    );
    assert_eq!(foreign_post.0, 403);
    assert_eq!(
        server
            .request(
                "POST",
                "/api/v1/tasks",
                r#"{"title":"forged"}"#,
                Some(&server.authority),
                None,
                None,
                Some("application/json")
            )
            .0,
        403
    );
    assert_eq!(
        server
            .request(
                "POST",
                "/api/v1/tasks",
                r#"{"title":"forged"}"#,
                Some(&server.authority),
                Some(&valid_origin),
                Some("cross-site"),
                Some("application/json")
            )
            .0,
        403
    );
    assert_eq!(
        server
            .request(
                "POST",
                "/api/v1/tasks",
                r#"{"title":"forged"}"#,
                Some(&server.authority),
                Some(&valid_origin),
                None,
                Some("text/plain")
            )
            .0,
        415
    );
    assert_eq!(server.json("POST", "/api/v1/tasks", "invalid json").0, 400);
    assert_eq!(
        server.json("POST", "/api/v1/tasks", r#"{"title":" "}"#).0,
        400
    );
    assert_eq!(
        server
            .json("POST", "/api/v1/tasks", r#"{"title":"valid","unknown":1}"#)
            .0,
        400
    );
    // Invalid revision representations must fail before record lookup.
    let path = format!("/api/v1/tasks/{}", Uuid::nil());
    let large = format!(r#"{{"title":"{}"}}"#, "x".repeat(20 * 1024));
    assert_eq!(server.json("POST", "/api/v1/tasks", &large).0, 413);
    assert_eq!(
        server
            .json("PATCH", &path, r#"{"revision":1,"completed":true}"#)
            .0,
        400
    );
    assert_eq!(
        server
            .json(
                "PATCH",
                &path,
                r#"{"revision":"18446744073709551616","completed":true}"#
            )
            .0,
        400
    );
    let (_status, body) = server.get("/api/v1/tasks");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["records"],
        json!([])
    );
}

#[cfg(unix)]
#[test]
fn ctrl_c_shuts_down_independent_backend_without_killing_browser_state() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("tasks.db");
    let mut server = Server::start(&db);
    assert_eq!(server.get("/api/v1/tasks").0, 200);
    assert!(
        Command::new("kill")
            .arg("-INT")
            .arg(server.child.id().to_string())
            .status()
            .unwrap()
            .success()
    );
    let end = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = server.child.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < end, "HTTP server ignored Ctrl-C");
        thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success(), "graceful HTTP shutdown failed: {status}");
}
