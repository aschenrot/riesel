use super::*;

struct FacadeHost(Child);

impl Drop for FacadeHost {
    fn drop(&mut self) {
        // Research harness owns only this exact spawned child.
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_client(address: SocketAddr, mode: &str) -> TaskResult {
    let output = output_with_timeout(
        Command::new(binary("counter_public_facade_client"))
            .arg(address.to_string())
            .arg(mode)
            .current_dir(workspace_root())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot start research facade client: {error}"))?,
        Duration::from_secs(12),
    )?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout)
            .contains(&format!("RESEARCH_FACADE_CLIENT_OK {mode}"))
    {
        return Err(format!(
            "independent facade client {mode} failed: status {}, stdout {}, stderr {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(())
}

/// Bound the first stdout announcement without hanging the smoke runner.
/// Restart can reuse the SAME address in the separate stale-client research.
fn start_facade_host(
    program: &str,
    bind_address: Option<SocketAddr>,
) -> Result<(FacadeHost, SocketAddr), String> {
    let mut command = Command::new(binary(program));
    command
        .current_dir(workspace_root())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env_remove("RIESEL_RESEARCH_FACADE_BIND");
    if let Some(address) = bind_address {
        command.env("RIESEL_RESEARCH_FACADE_BIND", address.to_string());
    }
    let mut host = FacadeHost(
        command
            .spawn()
            .map_err(|error| format!("cannot start {program} facade host: {error}"))?,
    );
    let stdout = host
        .0
        .stdout
        .take()
        .ok_or_else(|| "facade host did not pipe stdout".to_owned())?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut first = String::new();
        let result = BufReader::new(stdout)
            .read_line(&mut first)
            .map(|_| first)
            .map_err(|error| format!("cannot read facade bind announcement: {error}"));
        let _ = tx.send(result);
    });
    let announcement = rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|error| format!("facade host did not announce bounded startup: {error}"))??;
    reader
        .join()
        .map_err(|_| "facade readiness reader panicked".to_owned())?;
    let raw = announcement
        .trim()
        .strip_prefix("RESEARCH_FACADE_BOUND ")
        .ok_or_else(|| format!("unexpected facade bind announcement: {announcement}"))?;
    let address: SocketAddr = raw
        .parse()
        .map_err(|error| format!("invalid facade address: {error}"))?;
    if !address.ip().is_loopback() {
        return Err(format!(
            "facade host exposed a non-loopback address: {address}"
        ));
    }
    if let Some(expected) = bind_address {
        if address != expected {
            return Err(format!(
                "research restarted host changed address: expected {expected}, actual {address}"
            ));
        }
    }
    Ok((host, address))
}

fn stop_facade_host(host: &mut FacadeHost) -> TaskResult {
    host.0
        .stdin
        .take()
        .ok_or_else(|| "facade host control stdin is unavailable".to_owned())?
        .write_all(b"STOP\n")
        .map_err(|error| format!("cannot request facade host shutdown: {error}"))?;
    let status = wait_for(&mut host.0, Duration::from_secs(8))?;
    if !status.success() {
        return Err(format!(
            "research facade host failed to stop cleanly: {status}"
        ));
    }
    Ok(())
}

/// Research #75/#60: app-owned generated typed capability, an independent
/// process, deliberate facade selection, and backend survival after GUI exit.
/// Loopback RPC is unauthenticated; this is not an accepted public exporter.
fn verify_facade_host(program: &str) -> TaskResult {
    let (mut host, address) = start_facade_host(program, None)?;
    run_client(address, "first")?;
    // The headless app is still running after the first client OS process
    // exits. Second client sees state mutated by the first.
    if host
        .0
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("facade host exited when first client disconnected".into());
    }
    run_client(address, "second")?;
    stop_facade_host(&mut host)
}

/// Research NEGATIVE: an unchanged typed external client remains usable when
/// a completely new Riesel app process/session occupies the same address.
/// This is evidence of MISSING external generation fencing, NOT validation
/// of a future exporter, authentication or in-process session recovery.
fn verify_stale_external_client() -> TaskResult {
    let (mut first_host, address) = start_facade_host("counter_resolved_facade_host", None)?;
    let mut client = FacadeHost(
        Command::new(binary("counter_public_facade_client"))
            .arg(address.to_string())
            .arg("stale")
            .current_dir(workspace_root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("cannot start stale external client: {error}"))?,
    );
    let stdout = client
        .0
        .stdout
        .take()
        .ok_or_else(|| "stale external client did not pipe stdout".to_owned())?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx
                .send(line.map_err(|error| format!("stale client stdout: {error}")))
                .is_err()
            {
                break;
            }
        }
    });
    let first = rx
        .recv_timeout(Duration::from_secs(12))
        .map_err(|error| format!("stale client phase 1 timed out: {error}"))??;
    if first != "RESEARCH_FACADE_STALE_PHASE1_READY" {
        return Err(format!("unexpected stale client phase 1: {first}"));
    }

    // First resolved application state is now 5, satisfying the host's
    // exit check. Stop and reap that OS process before creating a fresh one.
    if client
        .0
        .try_wait()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("stale client exited before backend restart".into());
    }
    stop_facade_host(&mut first_host)?;
    let (mut second_host, rebound_address) =
        start_facade_host("counter_resolved_facade_host", Some(address))?;
    if rebound_address != address {
        return Err("fresh backend failed to reuse the expected endpoint".into());
    }

    // The SAME client object/process and generated binding are reused.
    // No re-verification or app-session handshake is performed.
    client
        .0
        .stdin
        .take()
        .ok_or_else(|| "stale external client control stdin unavailable".to_owned())?
        .write_all(b"CONTINUE\n")
        .map_err(|error| format!("cannot release stale client: {error}"))?;
    for expected in [
        "RESEARCH_FACADE_STALE_REBIND_ACCEPTED",
        "RESEARCH_FACADE_CLIENT_OK stale",
    ] {
        let got = rx
            .recv_timeout(Duration::from_secs(12))
            .map_err(|error| format!("stale client phase 2 timed out: {error}"))??;
        if got != expected {
            return Err(format!("stale client expected {expected:?}, got {got:?}"));
        }
    }
    let status = wait_for(&mut client.0, Duration::from_secs(8))?;
    if !status.success() {
        return Err(format!(
            "stale external client exited unsuccessfully: {status}"
        ));
    }
    reader
        .join()
        .map_err(|_| "stale client output reader panicked".to_owned())?;
    // The fresh backend received 2 + 3 from the stale client and holds 5.
    stop_facade_host(&mut second_host)?;
    println!("RESEARCH_FACADE_STALE_REBIND_PROVEN (unsafe baseline)");
    Ok(())
}

/// Compare hosting a selected component with exporting a capability already
/// resolved by the application runtime. Both use a headless external client.
pub(super) fn verify_two_process_facade() -> TaskResult {
    verify_facade_host("counter_public_facade_host")?;
    verify_facade_host("counter_resolved_facade_host")?;
    verify_stale_external_client()
}
