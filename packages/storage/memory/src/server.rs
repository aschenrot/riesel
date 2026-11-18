//! Storage's host implementation. The runtime-managed TCP lifecycle,
//! framing, readiness, and cooperative shutdown are shared by every RPC host.
use crate::memory_storage_provider;
use std::io;

pub fn run_storage_server(address: &str) -> io::Result<()> {
    memory_storage_provider()
        .map_err(io::Error::other)?
        .serve(address)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryStore, memory_provider_definition};
    use riesel_protocol::{
        CallError, RpcError, RpcHost, RpcTransport, TcpRpcTransport, handle_tcp_connection,
    };
    use riesel_storage::RecordStoreRpcDispatcher;
    use riesel_storage::{RecordStoreContract, RecordStoreRpcClient, StoreError};
    use serde_json::json;
    use std::net::TcpListener;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::thread;
    use std::time::{Duration, Instant};

    #[test]
    fn refuses_to_expose_unauthenticated_storage_on_any_interface() {
        let error = run_storage_server("0.0.0.0:0").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn generated_storage_remote_crud_and_error_round_trip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let stopping = Arc::new(AtomicBool::new(false));
        let server = {
            let stopping = Arc::clone(&stopping);
            thread::spawn(move || {
                let handler = RpcHost::new(
                    memory_provider_definition().unwrap(),
                    RecordStoreRpcDispatcher::new(MemoryStore::new()),
                )
                .unwrap();
                let deadline = Instant::now() + Duration::from_secs(8);
                loop {
                    if stopping.load(Ordering::Acquire) {
                        break;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "Storage RPC test host did not stop"
                    );
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            handle_tcp_connection(&handler, &mut stream).unwrap();
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(2));
                        }
                        Err(error) => panic!("Storage test host accept failed: {error}"),
                    }
                }
            })
        };

        let transport = TcpRpcTransport::new(address);
        let contract = RecordStoreContract::definition().unwrap();
        let actual = transport.describe(&contract.capability).unwrap();
        assert!(actual.has_same_api_shape(&contract));
        let provider = transport.provider(&contract.capability).unwrap();
        assert_eq!(provider.id, "riesel.storage.memory");
        assert!(provider.capabilities.contains(&contract.capability));

        let client = RecordStoreRpcClient::new(TcpRpcTransport::new(address));
        let created = client.create("task", json!({"title": "From RPC"})).unwrap();
        assert_eq!(created.revision, 1);
        let loaded = client.get("task", created.id).unwrap().unwrap();
        assert_eq!(loaded.id, created.id);
        let updated = client
            .update(
                "task",
                created.id,
                loaded.revision,
                json!({"title": "Updated"}),
            )
            .unwrap();
        assert_eq!(updated.revision, 2);
        let page = client.list_page("task", None, 32).unwrap();
        assert_eq!(page.records.len(), 1);
        assert!(page.next_cursor.is_none());
        assert_eq!(page.records[0].data, updated.data);

        assert!(matches!(
            client.update("task", created.id, 1, json!({"title": "Stale"})),
            Err(CallError::Domain(StoreError::RevisionConflict))
        ));
        assert!(client.get("task", uuid::Uuid::new_v4()).unwrap().is_none());

        let transport = TcpRpcTransport::new(address);
        assert!(matches!(
            transport.call(&contract.capability, "unknown", json!({})),
            Err(RpcError::UnknownOperation(_))
        ));
        let mut bad_version = contract.capability;
        bad_version.version += 1;
        assert!(matches!(
            transport.call(
                &bad_version,
                "list_page",
                json!({"model": "task", "after": null, "limit": 32}),
            ),
            Err(RpcError::WrongCapability { .. })
        ));
        client.delete("task", created.id, updated.revision).unwrap();
        assert!(client.get("task", created.id).unwrap().is_none());
        stopping.store(true, Ordering::Release);
        server.join().unwrap();
    }
}
