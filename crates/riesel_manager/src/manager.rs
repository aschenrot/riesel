use std::collections::HashMap;
use std::io;
use std::process::{Child, ChildStdout, Command, ExitStatus};

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
    start_order: Vec<ProcessKey>,
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
        self.processes.insert(key.clone(), ManagedProcess { child });
        self.start_order.push(key);
        Ok(())
    }

    /// Take the child's piped stdout for a bounded startup handshake.
    /// Requires the command to use Stdio::piped() before spawning.
    pub fn take_stdout(&mut self, key: &ProcessKey) -> io::Result<ChildStdout> {
        let process = self.processes.get_mut(key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("process not found: {key:?}"),
            )
        })?;
        process.child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "child stdout was not piped")
        })
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
        self.start_order.retain(|registered| registered != key);
        Ok(exit)
    }

    /// Process start order is the ownership order. Stop in reverse order;
    /// fail one child without skipping the rest.
    fn shutdown_order(&self) -> Vec<ProcessKey> {
        self.start_order.iter().rev().cloned().collect()
    }

    /// Exhaustively stop children in reverse registration order. Keep handles
    /// whose stop failed so a later cleanup attempt can retry them.
    pub fn stop_all(&mut self) -> io::Result<()> {
        let mut first_error = None;
        for key in self.shutdown_order() {
            if let Err(error) = self.stop(&key) {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

impl Drop for ProcessRegistry {
    fn drop(&mut self) {
        let _ = self.stop_all();
        // Last resort if a managed child failed the normal stop path.
        for key in self.shutdown_order() {
            if let Some(process) = self.processes.get_mut(&key) {
                let _ = process.child.kill();
                let _ = process.child.wait();
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
    fn stop_order_is_reverse_of_registration_even_with_unordered_keys() {
        let mut registry = ProcessRegistry::new();
        let first = ProcessKey::new(12, Some("first".into()));
        let second = ProcessKey::new(4, Some("second".into()));
        let third = ProcessKey::new(8, Some("third".into()));
        for key in [&first, &second, &third] {
            registry.spawn(key.clone(), &mut test_child()).unwrap();
        }
        assert_eq!(
            registry.shutdown_order(),
            vec![third.clone(), second.clone(), first.clone()]
        );
        registry.stop(&second).unwrap();
        assert_eq!(registry.shutdown_order(), vec![third, first]);
        registry.stop_all().unwrap();
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
