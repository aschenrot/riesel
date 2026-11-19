//! Bounded restart of complete application sessions, not individual live
//! providers. Reconstruct all capability handles after a managed child exits.
use crate::{AppRunError, ConnectorError, RunningApp, RuntimeError};
use serde::{Deserialize, Serialize};
use std::thread;
use std::time::Duration;

/// Default fail-fast policy or explicit bounded application-session restart.
/// Restart replays the entrypoint and recreates ephemeral providers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum RecoveryPolicy {
    #[default]
    FailFast,
    RestartApplication {
        max_restarts: u32,
        #[serde(default = "default_initial_backoff_ms")]
        initial_backoff_ms: u64,
        #[serde(default = "default_max_backoff_ms")]
        max_backoff_ms: u64,
    },
}

fn default_initial_backoff_ms() -> u64 {
    100
}
fn default_max_backoff_ms() -> u64 {
    5_000
}

impl RecoveryPolicy {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Self::RestartApplication {
            max_restarts,
            initial_backoff_ms,
            max_backoff_ms,
        } = self
        {
            if !(1..=32).contains(max_restarts) {
                return Err("restart policy requires 1..=32 permitted restarts");
            }
            if !(1..=120_000).contains(initial_backoff_ms)
                || !(1..=120_000).contains(max_backoff_ms)
                || initial_backoff_ms > max_backoff_ms
            {
                return Err(
                    "restart backoff must be positive, <= 120000 ms and initial <= maximum",
                );
            }
        }
        Ok(())
    }

    fn max_restarts(&self) -> u32 {
        match self {
            Self::FailFast => 0,
            Self::RestartApplication { max_restarts, .. } => *max_restarts,
        }
    }

    fn delay_for_next_restart(&self, already_restarted: u32) -> Duration {
        match self {
            Self::FailFast => Duration::ZERO,
            Self::RestartApplication {
                initial_backoff_ms,
                max_backoff_ms,
                ..
            } => Duration::from_millis(
                initial_backoff_ms
                    .saturating_mul(1_u64 << already_restarted.min(32))
                    .min(*max_backoff_ms),
            ),
        }
    }
}

/// Start clean sessions in dependency order after eligible managed-child
/// exits. Never retry business, shutdown or startup errors. Retrying would
/// risk replaying work or leaving resources owned by a prior session.
pub(crate) fn run_with_recovery(
    policy: &RecoveryPolicy,
    mut start: impl FnMut() -> Result<RunningApp, RuntimeError>,
    mut entrypoint: impl FnMut(&RunningApp) -> Result<(), ConnectorError>,
) -> Result<(), AppRunError> {
    let mut restarts = 0;
    loop {
        let running = start()?;
        let result = RunningApp::execute(running, &mut entrypoint);
        let Err(error) = result else {
            return Ok(());
        };
        // A native async entrypoint reports SessionExpired when a monitored
        // child fails. That is cancellation caused by supervision, not a
        // business error, and is eligible for the same opt-in whole-session
        // restart as a synchronous entrypoint that returned normally.
        // All other entrypoint errors remain ineligible: never replay an
        // arbitrary failed command or user handler.
        let eligible = matches!(
            &error,
            AppRunError::Supervision {
                failure,
                entry,
                shutdown: None,
            } if matches!(failure.as_ref(), RuntimeError::ServiceExited { .. })
                && entry.as_ref().is_none_or(|source| {
                    matches!(
                        source.downcast_ref::<riesel_protocol::RpcError>(),
                        Some(riesel_protocol::RpcError::SessionExpired)
                    )
                })
        );
        if !eligible {
            return Err(error);
        }
        let max_restarts = policy.max_restarts();
        if restarts == max_restarts {
            return if max_restarts == 0 {
                Err(error)
            } else {
                Err(AppRunError::RecoveryExhausted {
                    restarts,
                    last: Box::new(error),
                })
            };
        }
        thread::sleep(policy.delay_for_next_restart(restarts));
        restarts += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fail_fast_is_default_and_backoff_is_capped() {
        assert_eq!(
            serde_json::from_str::<RecoveryPolicy>(r#"{"mode":"fail_fast"}"#).unwrap(),
            RecoveryPolicy::FailFast
        );
        let policy = RecoveryPolicy::RestartApplication {
            max_restarts: 4,
            initial_backoff_ms: 10,
            max_backoff_ms: 35,
        };
        policy.validate().unwrap();
        assert_eq!(policy.delay_for_next_restart(0), Duration::from_millis(10));
        assert_eq!(policy.delay_for_next_restart(1), Duration::from_millis(20));
        assert_eq!(policy.delay_for_next_restart(2), Duration::from_millis(35));
        assert_eq!(policy.delay_for_next_restart(20), Duration::from_millis(35));
    }

    #[test]
    fn invalid_recovery_configuration_is_rejected() {
        for policy in [
            RecoveryPolicy::RestartApplication {
                max_restarts: 0,
                initial_backoff_ms: 1,
                max_backoff_ms: 10,
            },
            RecoveryPolicy::RestartApplication {
                max_restarts: 33,
                initial_backoff_ms: 1,
                max_backoff_ms: 10,
            },
            RecoveryPolicy::RestartApplication {
                max_restarts: 2,
                initial_backoff_ms: 0,
                max_backoff_ms: 10,
            },
            RecoveryPolicy::RestartApplication {
                max_restarts: 2,
                initial_backoff_ms: 11,
                max_backoff_ms: 10,
            },
        ] {
            assert!(policy.validate().is_err());
        }
    }
}
