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

/// Research #75/#60: app-owned generated typed capability, an independent
/// process, deliberate facade selection, and backend survival after GUI exit.
/// Loopback RPC is unauthenticated; this is not an accepted public exporter.
pub(super) fn verify_two_process_facade() -> TaskResult {
    let mut host = FacadeHost(
        Command::new(binary("counter_public_facade_host"))
            .current_dir(workspace_root())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("cannot start research facade host: {error}"))?,
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
        return Err(format!("facade host exposed a non-loopback address: {address}"));
    }

    run_client(address, "first")?;
    // The headless app is still running after the first client OS process
    // exits. Second client sees state mutated by the first.
    if host.0.try_wait().map_err(|error| error.to_string())?.is_some() {
        return Err("facade host exited when first client disconnected".into());
    }
    run_client(address, "second")?;

    host.0
        .stdin
        .take()
        .ok_or_else(|| "facade host control stdin is unavailable".to_owned())?
        .write_all(b"STOP\n")
        .map_err(|error| format!("cannot request facade host shutdown: {error}"))?;
    let status = wait_for(&mut host.0, Duration::from_secs(8))?;
    if !status.success() {
        return Err(format!("research facade host failed to stop cleanly: {status}"));
    }
    Ok(())
}
