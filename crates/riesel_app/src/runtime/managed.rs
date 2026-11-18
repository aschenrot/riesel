use super::*;

const MAX_READY_BYTES: u64 = 4 * 1024;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ShutdownMode {
    StdinLine,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServiceReady {
    pub(super) protocol: u32,
    pub(super) address: SocketAddr,
    #[serde(default)]
    pub(super) shutdown: Option<ShutdownMode>,
}

/// Read a single bounded startup frame from a private child stdout pipe.
/// Timeout kills the child through ProcessRegistry's drop cleanup.
pub(super) fn await_ready<R: Read + Send + 'static>(
    stdout: R,
    timeout: Duration,
) -> io::Result<ServiceReady> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let result = (|| {
            let mut limited = (&mut reader).take(MAX_READY_BYTES + 1);
            let mut message = Vec::new();
            limited.read_until(b'\n', &mut message)?;
            if message.last() != Some(&b'\n') || message.len() as u64 > MAX_READY_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "missing or oversized RIESEL_READY announcement",
                ));
            }
            let line = std::str::from_utf8(&message)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            let payload = line.strip_prefix("RIESEL_READY ").ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid startup announcement prefix",
                )
            })?;
            let ready: ServiceReady = serde_json::from_str(payload)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            if ready.protocol != 1 || !ready.address.ip().is_loopback() || ready.address.port() == 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported or non-loopback ready endpoint",
                ));
            }
            Ok(ready)
        })();
        let ready = result.is_ok();
        // Keep stdout alive after readiness so subsequent service logs do not
        // trigger a broken pipe. Drain it for the lifetime of the child.
        if sender.send(result).is_ok() && ready {
            let _ = io::copy(&mut reader, &mut io::stderr());
        }
    });
    receiver.recv_timeout(timeout).map_err(|error| {
        let kind = match error {
            mpsc::RecvTimeoutError::Timeout => io::ErrorKind::TimedOut,
            mpsc::RecvTimeoutError::Disconnected => io::ErrorKind::BrokenPipe,
        };
        io::Error::new(kind, "waiting for managed service readiness")
    })?
}
