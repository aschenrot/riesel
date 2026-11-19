use riesel::{Records, storage::RecordStore};
use riesel_storage_component::MemoryStore;
use serde_json::{Value, json};
use std::{io, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};
use todo::TaskData;

async fn start() -> (u16, oneshot::Sender<()>, JoinHandle<io::Result<()>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let store = Arc::new(MemoryStore::new()) as Arc<dyn RecordStore + Send + Sync>;
    let records = Records::<TaskData>::from_store(store);
    let (sender, receiver) = oneshot::channel();
    let server = tokio::spawn(async move {
        todo::http::serve(listener, records, async move {
            let _ = receiver.await;
        })
        .await
    });
    (port, sender, server)
}

fn decode_body(response: &str) -> String {
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    if !headers.to_ascii_lowercase().contains("transfer-encoding: chunked") {
        return body.to_string();
    }
    let mut remaining = body;
    let mut decoded = String::new();
    loop {
        let (size, rest) = remaining.split_once("\r\n").unwrap();
        let size = usize::from_str_radix(size.split(';').next().unwrap(), 16).unwrap();
        if size == 0 {
            break;
        }
        decoded.push_str(&rest[..size]);
        remaining = &rest[size + 2..];
    }
    decoded
}

async fn request(
    port: u16,
    method: &str,
    path: &str,
    origin: Option<&str>,
    host: Option<&str>,
    site: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let host = host.map(str::to_owned).unwrap_or_else(|| format!("127.0.0.1:{port}"));
    let mut wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n"
    );
    if let Some(origin) = origin {
        wire.push_str(&format!("Origin: {origin}\r\n"));
    }
    if let Some(site) = site {
        wire.push_str(&format!("Sec-Fetch-Site: {site}\r\n"));
    }
    if let Some(body) = body {
        wire.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    wire.push_str("\r\n");
    if let Some(body) = body {
        wire.push_str(body);
    }
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
        .await
        .unwrap()
        .unwrap();
    let response = String::from_utf8(response).unwrap();
    let status = response
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (status, decode_body(&response))
}

async fn same_origin(port: u16, method: &str, path: &str, body: Option<&str>) -> (u16, String) {
    let origin = format!("http://127.0.0.1:{port}");
    request(port, method, path, Some(&origin), None, None, body).await
}

#[tokio::test]
async fn task_crud_enforces_caller_revisions() {
    let (port, shutdown, worker) = start().await;

    let (status, body) = same_origin(port, "POST", "/api/tasks", Some(r#"{"title":"First"}"#)).await;
    assert_eq!(status, 201);
    let task: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(task["revision"], 1);
    assert_eq!(task["title"], "First");
    let id = task["id"].as_str().unwrap();

    let (status, body) = same_origin(port, "GET", "/api/tasks", None).await;
    assert_eq!(status, 200);
    let list: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);

    let path = format!("/api/tasks/{id}");
    let (status, body) = same_origin(
        port,
        "PATCH",
        &path,
        Some(r#"{"expected_revision":1,"title":"Edited"}"#),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["revision"], 2);

    let (status, body) = same_origin(
        port,
        "PATCH",
        &path,
        Some(r#"{"expected_revision":1,"completed":true}"#),
    )
    .await;
    assert_eq!(status, 409);
    assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["error"], "revision_conflict");

    let (status, body) = same_origin(
        port,
        "PATCH",
        &path,
        Some(r#"{"expected_revision":2,"completed":true}"#),
    )
    .await;
    assert_eq!(status, 200);
    let changed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(changed["revision"], 3);
    assert_eq!(changed["completed"], true);

    let (status, _) = same_origin(port, "DELETE", &path, Some(r#"{"expected_revision":2}"#)).await;
    assert_eq!(status, 409);
    let (status, _) = same_origin(port, "DELETE", &path, Some(r#"{"expected_revision":3}"#)).await;
    assert_eq!(status, 204);
    let (status, _) = same_origin(port, "GET", &path, None).await;
    assert_eq!(status, 404);

    shutdown.send(()).unwrap();
    worker.await.unwrap().unwrap();
    assert!(TcpStream::connect(("127.0.0.1", port)).await.is_err());
}

#[tokio::test]
async fn rejects_cross_site_writes_and_invalid_requests() {
    let (port, shutdown, worker) = start().await;
    let good_origin = format!("http://127.0.0.1:{port}");
    let body = r#"{"title":"Validated"}"#;

    let (status, problem) =
        request(port, "POST", "/api/tasks", None, None, None, Some(body)).await;
    assert_eq!(status, 403);
    assert_eq!(json!("invalid_origin"), serde_json::from_str::<Value>(&problem).unwrap()["error"]);

    let (status, _) = request(
        port, "POST", "/api/tasks", Some("https://other.example"), None, None, Some(body),
    ).await;
    assert_eq!(status, 403);

    let (status, _) = request(
        port, "POST", "/api/tasks", Some(&good_origin), Some("rebind.example"), None, Some(body),
    ).await;
    assert_eq!(status, 403);

    let (status, _) = request(
        port, "GET", "/api/tasks", None, None, Some("cross-site"), None,
    ).await;
    assert_eq!(status, 403);

    let (status, _) = request(
        port, "POST", "/api/tasks", Some(&good_origin), None, None, Some(r#"{"title":"   "}"#),
    ).await;
    assert_eq!(status, 422);

    let (status, _) = request(
        port, "POST", "/api/tasks", Some(&good_origin), None, None, Some(r#"{"title": "#),
    ).await;
    assert_eq!(status, 400);

    let (status, _) = request(
        port, "GET", "/api/tasks/not-a-uuid", None, None, None, None,
    ).await;
    assert_eq!(status, 400);

    let (status, body) =
        request(port, "GET", "/api/tasks", None, None, None, None).await;
    assert_eq!(status, 200);
    assert!(serde_json::from_str::<Vec<Value>>(&body).unwrap().is_empty());

    shutdown.send(()).unwrap();
    worker.await.unwrap().unwrap();
}
