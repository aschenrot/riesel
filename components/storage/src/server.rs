//! Storage's host implementation. The protocol, dispatcher and typed client
//! are generated/shared; no per-operation wire code is maintained here.
use crate::{MemoryStore, memory_provider_definition};
use riesel_protocol::{RpcHost, handle_tcp_connection};
use riesel_storage::RecordStoreRpcDispatcher;
use std::io::{self, Write};
use std::net::TcpListener;

pub fn run_storage_server(address: &str) -> io::Result<()> {
    let listener = TcpListener::bind(address)?;
    let handler = RpcHost::new(
        memory_provider_definition().map_err(io::Error::other)?,
        RecordStoreRpcDispatcher::new(MemoryStore::new()),
    )
    .map_err(io::Error::other)?;

    // The listener is already bound. Report the actual OS-assigned endpoint
    // through a bounded private startup pipe for managed local deployments.
    let ready = serde_json::json!({
        "protocol": 1,
        "address": listener.local_addr()?.to_string()
    });
    println!("RIESEL_READY {}", ready);
    io::stdout().flush()?;

    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                if let Err(error) = handle_tcp_connection(&handler, &mut stream) {
                    eprintln!("Storage request failed: {error}");
                }
            }
            Err(error) => eprintln!("Storage connection failed: {error}"),
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_protocol::{CallError, RpcError, RpcTransport, TcpRpcTransport};
    use riesel_storage::{RecordStoreContract, RecordStoreRpcClient, StoreError};
    use serde_json::json;
    use std::thread;

    #[test]
    fn generated_storage_remote_crud_and_error_round_trip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let handler = RpcHost::new(
                memory_provider_definition().unwrap(),
                RecordStoreRpcDispatcher::new(MemoryStore::new()),
            )
            .unwrap();
            // Inspect contract + provider, four normal operations, revision
            // conflict, missing record, unknown operation and wrong version.
            for _ in 0..10 {
                let (mut stream, _) = listener.accept().unwrap();
                handle_tcp_connection(&handler, &mut stream).unwrap();
            }
        });

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
        let all = client.list("task").unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].data, updated.data);

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
            transport.call(&bad_version, "list", json!({"model": "task"})),
            Err(RpcError::WrongCapability { .. })
        ));
        server.join().unwrap();
    }
}
