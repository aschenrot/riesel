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

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_storage::{RecordStore, RecordStoreContract, TcpStore};
    use serde_json::json;
    use std::thread;

    #[test]
    fn remote_storage_round_trip_verifies_contract_and_performs_crud() {
        // Bind port zero to avoid fixed ports and background services in tests.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let store = MemoryStore::new();
            let info = memory_service_info().unwrap();
            // Describe + Create + Get + Update + List.
            for _ in 0..5 {
                let (mut stream, _) = listener.accept().unwrap();
                handle_connection(&store, &info, &mut stream).unwrap();
            }
        });

        let client = TcpStore::new(address);
        let service_info = client.describe().unwrap();
        let contract = RecordStoreContract::definition().unwrap();
        TcpStore::verify_service_info(&service_info, "riesel.storage.memory", &contract.capability)
            .unwrap();

        assert!(
            TcpStore::verify_service_info(
                &service_info,
                "riesel.storage.wrong",
                &contract.capability,
            )
            .is_err()
        );

        let mut wrong_version = contract.capability.clone();
        wrong_version.version += 1;
        assert!(
            TcpStore::verify_service_info(&service_info, "riesel.storage.memory", &wrong_version,)
                .is_err()
        );

        let created = client.create("task", json!({"title":"From TCP"})).unwrap();
        assert_eq!(created.revision, 1);
        let loaded = client.get("task", created.id).unwrap().unwrap();
        assert_eq!(loaded.id, created.id);

        let updated = client
            .update(
                "task",
                created.id,
                loaded.revision,
                json!({"title":"Updated"}),
            )
            .unwrap();
        assert_eq!(updated.revision, 2);
        let all = client.list("task").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].data, updated.data);

        server.join().unwrap();
    }
}
