//! Research-only IPC transports with identical bounded length-prefixed frames.
//! Not an application exporter, authorization mechanism, or production API.
use futures_util::StreamExt;
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, ToFsName,
    tokio::{Stream as InterprocessStream, prelude::*},
};
use serde_json::{Value, json};
use std::{env, io, time::Duration};
use tipsy::{Endpoint, OnConflict, ServerId};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const MAX_REQUEST: usize = 128 * 1024;
const MAX_RESPONSE: usize = 1024 * 1024 + 4096;
const TIMEOUT: Duration = Duration::from_secs(3);

fn reply(request: &Value, generation: &str) -> Value {
    if request.get("wire_version").and_then(Value::as_u64) != Some(1) {
        return json!({"wire_version": 1, "status": "error", "code": "version_mismatch"});
    }
    match request.get("action").and_then(Value::as_str) {
        Some("describe") => json!({"wire_version":1, "status":"descriptor", "generation":generation}),
        Some("query") if request.get("generation").and_then(Value::as_str) == Some(generation) => {
            json!({"wire_version":1, "status":"ok", "value":{"state":"observed", "generation":generation}})
        }
        Some("query") => json!({"wire_version":1, "status":"error", "code":"stale_generation"}),
        _ => json!({"wire_version":1, "status":"error", "code":"unpublished_operation"}),
    }
}

async fn one_connection<S: AsyncRead + AsyncWrite + Unpin>(mut stream: S, generation: String) {
    let result: io::Result<()> = match tokio::time::timeout(TIMEOUT, async {
        let size = stream.read_u32().await? as usize;
        if size > MAX_REQUEST {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "over_budget"));
        }
        let mut bytes = vec![0; size];
        stream.read_exact(&mut bytes).await?;
        let request: Value = serde_json::from_slice(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let response = serde_json::to_vec(&reply(&request, &generation))
            .map_err(io::Error::other)?;
        if response.len() > MAX_RESPONSE {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "response_over_budget"));
        }
        stream.write_u32(response.len() as u32).await?;
        stream.write_all(&response).await?;
        stream.flush().await?;
        Ok(())
    }).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "frame timeout")),
    };
    if let Err(error) = result {
        eprintln!("bounded request rejected: {error}");
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err("usage: riesel-ipc-272 interprocess|tipsy ENDPOINT GENERATION".into());
    }
    let (kind, endpoint, generation) = (&args[1], &args[2], &args[3]);
    match kind.as_str() {
        "interprocess" => {
            // Intentionally avoid the library's default unlink-on-drop behavior.
            // A production adapter must preserve Riesel's existing endpoint ownership checks.
            let name = endpoint.as_str().to_fs_name::<GenericFilePath>()?;
            let listener = ListenerOptions::new()
                .name(name)
                .reclaim_name(false)
                .create_tokio()?;
            println!("READY {endpoint}");
            loop {
                let stream: InterprocessStream = listener.accept().await?;
                let generation = generation.to_owned();
                tokio::spawn(async move { one_connection(stream, generation).await });
            }
        }
        "tipsy" => {
            let endpoint = Endpoint::new(ServerId::new(endpoint.clone()), OnConflict::Error)?;
            let path = endpoint.path().to_string_lossy().into_owned();
            let mut incoming = Box::pin(endpoint.incoming()?);
            println!("READY {path}");
            while let Some(conn) = incoming.next().await {
                let stream = conn?;
                let generation = generation.to_owned();
                tokio::spawn(async move { one_connection(stream, generation).await });
            }
        }
        "tokio" => {
            #[cfg(unix)]
            {
                let listener = tokio::net::UnixListener::bind(endpoint)?;
                println!("READY {endpoint}");
                loop {
                    let (stream, _) = listener.accept().await?;
                    let generation = generation.to_owned();
                    tokio::spawn(async move { one_connection(stream, generation).await });
                }
            }
            #[cfg(windows)]
            {
                use tokio::net::windows::named_pipe::ServerOptions;
                // First-instance guard is only endpoint collision evidence, not caller identity.
                let mut server = ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(endpoint)?;
                println!("READY {endpoint}");
                loop {
                    server.connect().await?;
                    let connected = server;
                    // Create the next instance before handling the connected client.
                    server = ServerOptions::new().create(endpoint)?;
                    let generation = generation.to_owned();
                    tokio::spawn(async move { one_connection(connected, generation).await });
                }
            }
        }
        _ => return Err("unrecognized transport candidate".into()),
    }
    Ok(())
}
