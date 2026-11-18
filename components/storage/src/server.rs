use crate::dispatch::dispatch;
use crate::memory::MemoryStore;
use crate::memory_service_info;
use riesel_storage::{StorageMessage, StorageServiceInfo};

use std::io::{self, BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 64 * 1024;

pub fn run_storage_server(address: &str) -> io::Result<()> {
    let listener = TcpListener::bind(address)?;
    let store = MemoryStore::new();
    let info = memory_service_info().map_err(io::Error::other)?;

    println!("Storage listening on {}", listener.local_addr()?);

    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                if let Err(error) = handle_connection(&store, &info, &mut stream) {
                    eprintln!("Storage request failed: {error}");
                }
            }
            Err(error) => eprintln!("Connection failed: {error}"),
        }
    }

    Ok(())
}

fn handle_connection(
    store: &MemoryStore,
    info: &StorageServiceInfo,
    stream: &mut TcpStream,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;

    let request_bytes = read_request(stream)?;

    let message: StorageMessage =
        serde_json::from_slice(&request_bytes).map_err(io::Error::other)?;

    let response = dispatch(store, message, info);

    let response_bytes = serde_json::to_vec(&response).map_err(io::Error::other)?;

    stream.write_all(&response_bytes)?;
    stream.write_all(b"\n")?;

    Ok(())
}

fn read_request(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut reader = BufReader::new(stream);
    let mut request = Vec::new();

    loop {
        let buffer = reader.fill_buf()?;

        if buffer.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "incomplete storage request",
            ));
        }

        let end = buffer
            .iter()
            .position(|&byte| byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(buffer.len());

        if request.len() + end > MAX_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "storage request too large",
            ));
        }

        request.extend_from_slice(&buffer[..end]);
        reader.consume(end);

        if request.last() == Some(&b'\n') {
            return Ok(request);
        }
    }
}
