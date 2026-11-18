use std::collections::HashMap;
use std::io;
use std::process::{Child, Command, ExitStatus};

/// Uniquely identifies a process instance owned by this registry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProcessKey {
    pub instance_id: u64,
    pub name: Option<String>,
}

impl ProcessKey {
    pub fn new(instance_id: u64, name: Option<String>) -> Self {
        Self { instance_id, name }
    }
}

#[derive(Debug)]
pub enum ProcessStatus {
    Running,
    Exited(ExitStatus),
}

struct ManagedProcess {
    child: Child,
}

/// Owns explicitly launched child processes. This is a local process
/// lifecycle primitive, not a provider resolver or a restart supervisor.
#[derive(Default)]
pub struct ProcessRegistry {
    processes: HashMap<ProcessKey, ManagedProcess>,
}

impl ProcessRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn an explicitly configured command. The caller remains responsible
    /// for selecting a trusted executable and checking service readiness.
    pub fn spawn(&mut self, key: ProcessKey, command: &mut Command) -> io::Result<()> {
        if self.processes.contains_key(&key) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("process already registered: {key:?}"),
            ));
        }
        let child = command.spawn()?;
        self.processes.insert(key, ManagedProcess { child });
        Ok(())
    }

    /// An exited child remains registered until stop() or stop_all().
    pub fn status(&mut self, key: &ProcessKey) -> io::Result<ProcessStatus> {
        let process = self.processes.get_mut(key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("process not found: {key:?}"),
            )
        })?;

        Ok(match process.child.try_wait()? {
            Some(status) => ProcessStatus::Exited(status),
            None => ProcessStatus::Running,
        })
    }

    /// Force-stop and reap a child, or reap it if it has already exited.
    pub fn stop(&mut self, key: &ProcessKey) -> io::Result<ExitStatus> {
        let process = self.processes.get_mut(key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("process not found: {key:?}"),
            )
        })?;
        let exit = match process.child.try_wait()? {
            Some(status) => status,
            None => {
                process.child.kill()?;
                process.child.wait()?
            }
        };
        self.processes.remove(key);
        Ok(exit)
    }

    /// Stop every registered child; leave a failed entry registered so that
    /// cleanup can be retried rather than silently losing its handle.
    pub fn stop_all(&mut self) -> io::Result<()> {
        for key in self.processes.keys().cloned().collect::<Vec<_>>() {
            self.stop(&key)?;
        }
        Ok(())
    }
}

impl Drop for ProcessRegistry {
    fn drop(&mut self) {
        for process in self.processes.values_mut() {
            // Best-effort cleanup on scope exit. Explicit stop() reports errors.
            match process.child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    let _ = process.child.kill();
                    let _ = process.child.wait();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn child_waits_for_shutdown() {
        if std::env::var_os("RIESEL_PROCESS_MANAGER_TEST_CHILD").is_some() {
            std::thread::sleep(Duration::from_secs(30));
        }
    }

    fn test_child() -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "manager::tests::child_waits_for_shutdown"])
            .env("RIESEL_PROCESS_MANAGER_TEST_CHILD", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    #[test]
    fn starts_and_stops_explicit_child_process() {
        let key = ProcessKey::new(7, Some("test-child".into()));
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut test_child()).unwrap();

        assert!(matches!(
            registry.status(&key).unwrap(),
            ProcessStatus::Running
        ));
        assert_eq!(
            registry
                .spawn(key.clone(), &mut test_child())
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );

        registry.stop(&key).unwrap();
        assert_eq!(
            registry.status(&key).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn stops_all_children() {
        let mut registry = ProcessRegistry::new();
        registry
            .spawn(ProcessKey::new(1, None), &mut test_child())
            .unwrap();
        registry
            .spawn(ProcessKey::new(2, None), &mut test_child())
            .unwrap();
        registry.stop_all().unwrap();
        assert_eq!(
            registry
                .status(&ProcessKey::new(1, None))
                .unwrap_err()
                .kind(),
            io::ErrorKind::NotFound
        );
    }
}
