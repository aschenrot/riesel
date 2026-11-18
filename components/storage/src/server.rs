//! Storage's host implementation. The runtime-managed TCP lifecycle,
//! framing, readiness, and cooperative shutdown are shared by every RPC host.
use crate::{MemoryStore, memory_provider_definition};
use riesel_protocol::{RpcHost, run_tcp_rpc_host};
use riesel_storage::RecordStoreRpcDispatcher;
use std::io;

pub fn run_storage_server(address: &str) -> io::Result<()> {
    let handler = RpcHost::new(
        memory_provider_definition().map_err(io::Error::other)?,
        RecordStoreRpcDispatcher::new(MemoryStore::new()),
    )
    .map_err(io::Error::other)?;
    run_tcp_rpc_host(address, &handler)
}
#[cfg(test)]
mod tests {
    use super::*;
    use riesel_protocol::{CallError, RpcError, RpcTransport, TcpRpcTransport};
    use riesel_storage::{RecordStoreContract, RecordStoreRpcClient, StoreError};
    use serde_json::json;
    use std::net::TcpListener;
    use std::thread;

    #[test]
    fn refuses_to_expose_unauthenticated_storage_on_any_interface() {
        let error = run_storage_server("0.0.0.0:0").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

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
