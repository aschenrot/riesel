use super::*;

struct ObservedConnector {
    rpc: RpcConnector<RecordStoreContract>,
    connected: Arc<Mutex<Vec<SocketAddr>>>,
}

impl CapabilityConnector for ObservedConnector {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        self.rpc.contract()
    }

    fn connect(
        &self,
        endpoint: &ServiceEndpoint,
        session: &riesel::SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        let connected = self.rpc.connect(endpoint, session)?;
        if let Some(address) = endpoint.address() {
            self.connected.lock().unwrap().push(address);
        }
        Ok(connected)
    }
}

/// A real first managed Storage process becomes ready; the next dependent
/// process cannot spawn. All previously started children must be reaped.
pub(super) fn verify_multi_service_rollback() -> TaskResult {
    let app = App::from_schema(
        r#"{
            "version":1,
            "app":{"name":"RollbackProbe"},
            "models":{},
            "requires":[
                {"name":"second","capability":{"id":"riesel.storage.records","version":1},
                 "depends_on":["first"]},
                {"name":"first","capability":{"id":"riesel.storage.records","version":1}}
            ]
        }"#,
    )
    .map_err(|error| error.to_string())?;
    let missing_program = workspace_root().join("riesel-nonexistent-service-for-rollback");
    if missing_program.exists() {
        return Err("rollback test executable unexpectedly exists".into());
    }
    let deployment = serde_json::json!({
        "services": {
            "first": {"transport":"managed","program":binary("storage_service")},
            "second": {"transport":"managed","program":missing_program}
        }
    });
    let config =
        DeploymentConfig::from_json(&deployment.to_string()).map_err(|error| error.to_string())?;
    let connected = Arc::new(Mutex::new(Vec::new()));
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(ObservedConnector {
            rpc: RpcConnector::new(),
            connected: Arc::clone(&connected),
        })
        .map_err(|error| error.to_string())?;

    match app.start(&config, &connectors) {
        Err(RuntimeError::ServiceStart { requirement, .. }) if requirement == "second" => {}
        Err(error) => return Err(format!("unexpected rollback failure: {error}")),
        Ok(mut running) => {
            running.shutdown().map_err(|error| error.to_string())?;
            return Err("expected failure to spawn second managed service".into());
        }
    }

    let addresses = connected.lock().map_err(|error| error.to_string())?;
    let [address] = addresses.as_slice() else {
        return Err(format!(
            "expected one verified live service, found {}",
            addresses.len()
        ));
    };
    if TcpStream::connect_timeout(address, Duration::from_millis(250)).is_ok() {
        return Err(format!(
            "first managed service survived rollback at {address}"
        ));
    }
    Ok(())
}

/// Kill and reap the child even if readiness, protocol validation, or graceful
/// shutdown fails partway through a smoke test.
struct ChildCleanup(Child);

impl Drop for ChildCleanup {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn verify_graceful_storage_exit() -> TaskResult {
    let mut child = ChildCleanup(
        Command::new(binary("storage_service"))
            .env("RIESEL_BIND_ADDRESS", "127.0.0.1:0")
            .env("RIESEL_CONTROL_STDIN", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot spawn cooperative Storage: {error}"))?,
    );

    let stdout = child
        .0
        .stdout
        .take()
        .ok_or("managed Storage stdout not piped")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let ready_line = receiver
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| format!("Storage readiness timed out: {error}"))?
        .map_err(|error| format!("Storage readiness pipe failed: {error}"))?;
    let payload = ready_line
        .strip_prefix("RIESEL_READY ")
        .ok_or("Storage omitted RIESEL_READY startup frame")?;
    let ready: serde_json::Value = serde_json::from_str(payload)
        .map_err(|error| format!("invalid Storage ready frame: {error}"))?;
    if ready["shutdown"] != "stdin_line" || !ready["address"].is_string() {
        return Err(format!(
            "Storage did not negotiate stdin_line shutdown: {ready}"
        ));
    }

    child
        .0
        .stdin
        .take()
        .ok_or("managed Storage stdin not piped")?
        .write_all(b"RIESEL_SHUTDOWN\n")
        .map_err(|error| format!("cannot request Storage shutdown: {error}"))?;

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.0.try_wait().map_err(|error| error.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!(
                    "cooperative Storage exited unsuccessfully: {status}"
                ))
            };
        }
        if Instant::now() >= deadline {
            return Err("Storage did not exit after cooperative shutdown request".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

