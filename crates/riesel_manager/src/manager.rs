use std::collections::HashMap;
use std::io::{self, Write};
use std::process::{Child, ChildStdout, Command, ExitStatus};
#[cfg(feature = "internal-observation")]
use std::sync::{
    Arc, Weak,
    atomic::{AtomicU8, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

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
    graceful_timeout: Option<Duration>,
    #[cfg(feature = "internal-observation")]
    attempt: Arc<AtomicU8>,
}

/// The local ProcessRegistry has a real OS child handle for this observation,
/// not a globally unique process attempt ID or a protected Manager grant.
#[cfg(feature = "internal-observation")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedAttemptPhase {
    Running,
    ExitedSuccess,
    ExitedFailure,
}

#[cfg(feature = "internal-observation")]
impl OwnedAttemptPhase {
    fn value(self) -> u8 {
        match self {
            Self::Running => 0,
            Self::ExitedSuccess => 1,
            Self::ExitedFailure => 2,
        }
    }
}

/// A read-only, attempt-scoped snapshot of a child owned by *this* registry.
///
/// Retains a Weak lifetime fence so removing or replacing the same ProcessKey
/// can never promote the earlier attempt. No process handle or OS command is
/// exposed; future aliveness, PID provenance, reattachment after owner death
/// and remote principal identity are deliberately not claimed.
#[cfg(feature = "internal-observation")]
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct OwnedAttemptEvidence {
    key: ProcessKey,
    phase: OwnedAttemptPhase,
    observed_at: Instant,
    owner: Weak<AtomicU8>,
}

#[cfg(feature = "internal-observation")]
impl OwnedAttemptEvidence {
    pub fn key(&self) -> &ProcessKey {
        &self.key
    }

    pub fn phase(&self) -> OwnedAttemptPhase {
        self.phase
    }

    pub fn observed_at(&self) -> Instant {
        self.observed_at
    }

    pub fn same_owned_attempt_as(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.owner, &other.owner)
    }

    /// Reading a snapshot cannot poll the OS a second time. Its evidence
    /// becomes stale when the registry drops/replaces the owned process,
    /// when a later owner poll witnesses a different phase, or on timeout.
    pub fn is_current_at(&self, now: Instant, max_age: Duration) -> bool {
        if now < self.observed_at || now.duration_since(self.observed_at) > max_age {
            return false;
        }
        self.owner
            .upgrade()
            .is_some_and(|owner| owner.load(Ordering::Acquire) == self.phase.value())
    }
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
        self.processes.insert(
            key.clone(),
            ManagedProcess {
                child,
                graceful_timeout: None,
                #[cfg(feature = "internal-observation")]
                attempt: Arc::new(AtomicU8::new(OwnedAttemptPhase::Running.value())),
            },
        );
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

    /// Enable the private stdin shutdown protocol only after the managed child
    /// explicitly advertises support in its readiness handshake.
    pub fn enable_graceful_shutdown(
        &mut self,
        key: &ProcessKey,
        timeout: Duration,
    ) -> io::Result<()> {
        let process = self.processes.get_mut(key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("process not found: {key:?}"),
            )
        })?;
        if process.child.stdin.is_none() || timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "graceful shutdown requires piped stdin and a positive timeout",
            ));
        }
        process.graceful_timeout = Some(timeout);
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

        let status = match process.child.try_wait()? {
            Some(status) => ProcessStatus::Exited(status),
            None => ProcessStatus::Running,
        };
        #[cfg(feature = "internal-observation")]
        if let ProcessStatus::Exited(exit) = &status {
            let phase = if exit.success() {
                OwnedAttemptPhase::ExitedSuccess
            } else {
                OwnedAttemptPhase::ExitedFailure
            };
            process.attempt.store(phase.value(), Ordering::Release);
        }
        Ok(status)
    }

    /// Feature-gated evidence from this registry's **actual retained Child**
    /// handle. A missing key or OS query failure remains an error, not
    /// synthesized proof that a process stopped. No global attempt token.
    #[cfg(feature = "internal-observation")]
    #[doc(hidden)]
    pub fn observe_owned_attempt(&mut self, key: &ProcessKey) -> io::Result<OwnedAttemptEvidence> {
        let phase = match self.status(key)? {
            ProcessStatus::Running => OwnedAttemptPhase::Running,
            ProcessStatus::Exited(exit) if exit.success() => OwnedAttemptPhase::ExitedSuccess,
            ProcessStatus::Exited(_) => OwnedAttemptPhase::ExitedFailure,
        };
        let process = self
            .processes
            .get(key)
            .expect("status verified the same registry key");
        Ok(OwnedAttemptEvidence {
            key: key.clone(),
            phase,
            observed_at: Instant::now(),
            owner: Arc::downgrade(&process.attempt),
        })
    }

    /// Ask an opted-in child to exit cleanly, wait for a bounded deadline,
    /// then force-kill and reap it if it does not cooperate.
    pub fn stop(&mut self, key: &ProcessKey) -> io::Result<ExitStatus> {
        let process = self.processes.get_mut(key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("process not found: {key:?}"),
            )
        })?;
        let mut exit = process.child.try_wait()?;
        if exit.is_none()
            && let Some(timeout) = process.graceful_timeout
        {
            let notified = process
                .child
                .stdin
                .take()
                .is_some_and(|mut input| input.write_all(b"RIESEL_SHUTDOWN\n").is_ok());
            if notified {
                let deadline = Instant::now() + timeout;
                while Instant::now() < deadline {
                    if let Some(status) = process.child.try_wait()? {
                        exit = Some(status);
                        break;
                    }
                    thread::sleep(Duration::from_millis(10));
                }
            }
        }
        // A child can exit at the deadline or between polling iterations.
        let exit = match exit.or(process.child.try_wait()?) {
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
            if let Err(error) = self.stop(&key)
                && first_error.is_none()
            {
                first_error = Some(error);
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
        if std::env::var_os("RIESEL_PROCESS_TEST_EXIT_ON_STDIN").is_some() {
            let mut release = String::new();
            std::io::stdin().read_line(&mut release).unwrap();
            assert_eq!(release, "FINISH\n");
            return;
        }
        if std::env::var_os("RIESEL_PROCESS_MANAGER_TEST_CHILD").is_some() {
            if std::env::var_os("RIESEL_PROCESS_GRACEFUL").is_some() {
                let mut request = String::new();
                std::io::stdin().read_line(&mut request).unwrap();
                assert_eq!(request, "RIESEL_SHUTDOWN\n");
            } else {
                std::thread::sleep(Duration::from_secs(30));
            }
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

    /// An exact owned Child is a credible local process observation.
    /// Re-using a *display/key* after an actual stop is not the same attempt.
    #[cfg(feature = "internal-observation")]
    #[test]
    fn attempt_evidence_stales_on_owner_stop_and_same_key_reuse() {
        let key = ProcessKey::new(101, Some("counter".into()));
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut test_child()).unwrap();
        let old = registry.observe_owned_attempt(&key).unwrap();
        assert_eq!(old.phase(), OwnedAttemptPhase::Running);
        assert!(old.is_current_at(Instant::now(), Duration::from_secs(5)));

        registry.stop(&key).unwrap();
        assert!(!old.is_current_at(Instant::now(), Duration::from_secs(5)));
        assert_eq!(
            registry.observe_owned_attempt(&key).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );

        // Same key, even same program/PID space, cannot revalidate A.
        registry.spawn(key.clone(), &mut test_child()).unwrap();
        let current = registry.observe_owned_attempt(&key).unwrap();
        assert!(!old.same_owned_attempt_as(&current));
        assert_eq!(current.key(), &key);
        assert_eq!(current.phase(), OwnedAttemptPhase::Running);
        assert!(current.is_current_at(Instant::now(), Duration::from_secs(5)));
        assert!(!old.is_current_at(Instant::now(), Duration::from_secs(5)));
        assert!(!current.is_current_at(
            current.observed_at() + Duration::from_secs(5) + Duration::from_nanos(1),
            Duration::from_secs(5)
        ));
        registry.stop_all().unwrap();
    }

    /// An OS child truly exits; status probing observes that exact event,
    /// invalidating prior running evidence without guessing from a missing PID.
    #[cfg(feature = "internal-observation")]
    #[test]
    fn real_child_exit_invalidates_prior_running_evidence() {
        let key = ProcessKey::new(102, Some("oneshot".into()));
        let mut command = test_child();
        command
            .env("RIESEL_PROCESS_TEST_EXIT_ON_STDIN", "1")
            .stdin(Stdio::piped());
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut command).unwrap();
        let first = registry.observe_owned_attempt(&key).unwrap();
        assert_eq!(first.phase(), OwnedAttemptPhase::Running);
        // Child remains blocked on its private stdin, so a startup delay
        // cannot cause an accidental already-exited first observation.
        let mut release = registry
            .processes
            .get_mut(&key)
            .unwrap()
            .child
            .stdin
            .take()
            .unwrap();
        release.write_all(b"FINISH\n").unwrap();
        drop(release);
        let deadline = Instant::now() + Duration::from_secs(4);
        let exited = loop {
            let observed = registry.observe_owned_attempt(&key).unwrap();
            if observed.phase() == OwnedAttemptPhase::ExitedSuccess {
                break observed;
            }
            assert!(
                Instant::now() < deadline,
                "actual child test executable did not exit within four seconds"
            );
            thread::sleep(Duration::from_millis(10));
        };
        assert!(!first.is_current_at(Instant::now(), Duration::from_secs(5)));
        assert!(exited.is_current_at(Instant::now(), Duration::from_secs(5)));
        assert!(first.same_owned_attempt_as(&exited));
        registry.stop(&key).unwrap();
        assert!(!exited.is_current_at(Instant::now(), Duration::from_secs(5)));
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
        for key in [first.clone(), second.clone(), third.clone()] {
            registry.spawn(key, &mut test_child()).unwrap();
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

    #[test]
    fn cooperative_child_exits_cleanly_without_force_kill() {
        let key = ProcessKey::new(21, Some("cooperative".into()));
        let mut command = test_child();
        command
            .stdin(Stdio::piped())
            .env("RIESEL_PROCESS_GRACEFUL", "1");
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut command).unwrap();
        registry
            .enable_graceful_shutdown(&key, Duration::from_secs(5))
            .unwrap();
        assert!(registry.stop(&key).unwrap().success());
    }

    #[test]
    fn non_cooperative_child_is_force_killed_after_deadline() {
        let key = ProcessKey::new(22, Some("unresponsive".into()));
        let mut command = test_child();
        command.stdin(Stdio::piped());
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut command).unwrap();
        registry
            .enable_graceful_shutdown(&key, Duration::from_millis(30))
            .unwrap();
        assert!(!registry.stop(&key).unwrap().success());
    }

    #[test]
    fn graceful_shutdown_requires_piped_stdin() {
        let key = ProcessKey::new(23, None);
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut test_child()).unwrap();
        assert_eq!(
            registry
                .enable_graceful_shutdown(&key, Duration::from_millis(10))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        registry.stop(&key).unwrap();
    }
}
