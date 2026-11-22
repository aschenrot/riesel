//! Optional T1 local AF_UNIX read attachment, deliberately separate from GUI
//! rendering, Manager authority and protected individual client identity.
//!
//! Same-OS-user peers are *trusted by policy*. The socket path, file mode,
//! descriptor fields and session UUID cannot authenticate a hostile peer.
use crate::{AppReadPublication, ReadPublicationDescriptor, ReadPublicationError};
use serde_json::{Value, json};
use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::task::JoinSet;

const MAX_CLIENTS: usize = 16;
const MAX_WIRE_REQUEST_BYTES: u64 = 128 * 1024;
const MAX_WIRE_RESPONSE_BYTES: usize = 1024 * 1024 + 4096;
const WIRE_VERSION: u64 = 1;
const IO_TIMEOUT: Duration = Duration::from_secs(4);

/// Explicit app-selected local transport. The caller must place its socket
/// in a deliberately trusted local directory and run `serve` as an owned
/// Riesel application task. No fixed port or process-wide registry.
pub struct UnixReadServer {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl UnixReadServer {
    pub fn bind(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "T1 socket path must be absolute",
            ));
        }
        // Do not unlink a preexisting socket, symlink or unrelated file.
        // This is only an accidental collision check: same-UID attackers
        // are in the explicitly assumed trusted group for T1.
        if fs::symlink_metadata(path).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "refusing to replace an existing attachment endpoint",
            ));
        }
        let listener = UnixListener::bind(path)?;
        if let Err(error) = fs::set_permissions(path, fs::Permissions::from_mode(0o600)) {
            let _ = fs::remove_file(path);
            return Err(error);
        }
        let metadata = fs::symlink_metadata(path)?;
        Ok(Self {
            listener,
            path: path.to_path_buf(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Drop/abort of the application-owned future drops the listener and
    /// the bounded request task set. Application session expiration is
    /// checked even if a caller fails to use Riesel's owned AsyncTasks.
    pub async fn serve(self, publication: Arc<AppReadPublication>) -> io::Result<()> {
        let mut workers = JoinSet::new();
        while publication.session_is_active() {
            while let Some(result) = workers.try_join_next() {
                if result.is_err() {
                    // A failed individual read is isolated from other clients.
                }
            }
            match tokio::time::timeout(Duration::from_millis(100), self.listener.accept()).await {
                Ok(Ok((mut stream, _))) => {
                    if workers.len() >= MAX_CLIENTS {
                        let _ = tokio::time::timeout(
                            IO_TIMEOUT,
                            stream.write_all(
                                b"{\"wire_version\":1,\"status\":\"error\",\"code\":\"at_capacity\"}",
                            ),
                        )
                        .await;
                        continue;
                    }
                    let publication = Arc::clone(&publication);
                    workers.spawn(async move { serve_one(stream, publication).await });
                }
                Ok(Err(error)) => {
                    workers.abort_all();
                    while workers.join_next().await.is_some() {}
                    return Err(error);
                }
                Err(_) => {}
            }
        }
        workers.abort_all();
        while workers.join_next().await.is_some() {}
        Ok(())
    }
}

impl Drop for UnixReadServer {
    fn drop(&mut self) {
        // An application must not delete an unrelated replacement endpoint.
        // T1 does not promise protection against concurrent hostile same-UID
        // path replacement between this check and unlink.
        if let Ok(meta) = fs::symlink_metadata(&self.path) {
            if meta.file_type().is_socket()
                && meta.dev() == self.device
                && meta.ino() == self.inode
            {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
}

async fn serve_one(mut stream: UnixStream, publication: Arc<AppReadPublication>) {
    let mut bytes = Vec::new();
    let outcome = {
        let mut reader = (&mut stream).take(MAX_WIRE_REQUEST_BYTES + 1);
        tokio::time::timeout(IO_TIMEOUT, reader.read_to_end(&mut bytes)).await
    };
    let response = match outcome {
        Ok(Ok(_)) if bytes.len() as u64 <= MAX_WIRE_REQUEST_BYTES => {
            match serde_json::from_slice::<Value>(&bytes) {
                Ok(request) => respond(&publication, request).await,
                Err(_) => problem("invalid_request"),
            }
        }
        Ok(Ok(_)) => problem("over_budget"),
        _ => problem("request_timeout_or_io_failure"),
    };
    let mut response_bytes = match serde_json::to_vec(&response) {
        Ok(body) if body.len() <= MAX_WIRE_RESPONSE_BYTES => body,
        _ => serde_json::to_vec(&problem("response_over_budget")).unwrap(),
    };
    response_bytes.push(b'\n');
    let _ = tokio::time::timeout(IO_TIMEOUT, stream.write_all(&response_bytes)).await;
    let _ = stream.shutdown().await;
}

fn problem(code: &str) -> Value {
    json!({"wire_version": WIRE_VERSION, "status": "error", "code": code})
}

async fn respond(publication: &AppReadPublication, request: Value) -> Value {
    if request.get("wire_version").and_then(Value::as_u64) != Some(WIRE_VERSION) {
        return problem("unsupported_wire_version");
    }
    match request.get("action").and_then(Value::as_str) {
        Some("describe") => {
            if request.as_object().is_none_or(|v| v.len() != 2) {
                return problem("invalid_request");
            }
            // Explicitly untrusted announcement. A same-user program could
            // impersonate this endpoint after rebinding its path.
            json!({
                "wire_version": WIRE_VERSION,
                "status": "descriptor",
                "descriptor": publication.descriptor()
            })
        }
        Some("query") => {
            let Some(fields) = request.as_object() else {
                return problem("invalid_request");
            };
            if fields.len() != 5 {
                return problem("invalid_request");
            }
            let Some(operation) = request.get("operation").and_then(Value::as_str) else {
                return problem("invalid_request");
            };
            let Some(descriptor) = request.get("descriptor").cloned().and_then(|v| {
                serde_json::from_value::<ReadPublicationDescriptor>(v).ok()
            }) else {
                return problem("invalid_request");
            };
            let Some(arguments) = request.get("arguments").cloned() else {
                return problem("invalid_request");
            };
            match tokio::time::timeout(
                IO_TIMEOUT,
                publication.query(&descriptor, operation, arguments),
            )
            .await
            {
                Ok(Ok(value)) => json!({
                    "wire_version": WIRE_VERSION, "status": "ok", "value": value
                }),
                Ok(Err(error)) => problem(match error {
                    ReadPublicationError::WrongInterface => "wrong_interface",
                    ReadPublicationError::WrongContract => "wrong_contract",
                    ReadPublicationError::StaleGeneration => "stale_generation",
                    ReadPublicationError::WrongPublication => "wrong_publication",
                    ReadPublicationError::Withdrawn => "withdrawn",
                    ReadPublicationError::NotPublished
                    | ReadPublicationError::NotReadOnly(_) => "not_published",
                    ReadPublicationError::AtCapacity => "at_capacity",
                    ReadPublicationError::Expired => "expired",
                    ReadPublicationError::OverBudget => "over_budget",
                    ReadPublicationError::Invalid(_) => "invalid_request",
                    ReadPublicationError::Rpc(_) => "domain_or_rpc_error",
                }),
                Err(_) => problem("read_timeout"),
            }
        }
        _ => problem("unknown_action"),
    }
}

/// A tiny, typed native-client adapter over the same read-only protocol.
/// No automatic session rebind or remote client identity is implied.
pub struct UnixReadClient {
    path: PathBuf,
}

impl UnixReadClient {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    async fn exchange(&self, request: Value) -> io::Result<Value> {
        let body = serde_json::to_vec(&request)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        if body.len() as u64 > MAX_WIRE_REQUEST_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "request too large"));
        }
        let mut stream = tokio::time::timeout(IO_TIMEOUT, UnixStream::connect(&self.path))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "local connect timeout"))??;
        tokio::time::timeout(IO_TIMEOUT, stream.write_all(&body))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "local write timeout"))??;
        stream.shutdown().await?;
        let mut reply = Vec::new();
        let mut reader = stream.take((MAX_WIRE_RESPONSE_BYTES + 1) as u64);
        tokio::time::timeout(IO_TIMEOUT, reader.read_to_end(&mut reply))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "local read timeout"))??;
        if reply.len() > MAX_WIRE_RESPONSE_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "response too large"));
        }
        serde_json::from_slice(&reply)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub async fn describe(&self) -> io::Result<ReadPublicationDescriptor> {
        let reply = self.exchange(json!({
            "wire_version": WIRE_VERSION, "action": "describe"
        })).await?;
        if reply["status"] != "descriptor" {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "no descriptor"));
        }
        serde_json::from_value(reply["descriptor"].clone())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// An outcome code is returned verbatim in the JSON error reply.
    /// The client must explicitly re-describe/resnapshot after changed
    /// session generation; there is no transparent rebind.
    pub async fn query(
        &self,
        descriptor: &ReadPublicationDescriptor,
        operation: &str,
        arguments: Value,
    ) -> io::Result<Value> {
        self.exchange(json!({
            "wire_version": WIRE_VERSION,
            "action": "query",
            "descriptor": descriptor,
            "operation": operation,
            "arguments": arguments
        })).await
    }
}
