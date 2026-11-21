//! Deterministic fixed-step budget accounting for one chosen clock owner.
//!
//! This module does not schedule Tokio tasks, invoke graph handlers, own the
//! underlying domain clock, or certify that an operation has committed.
//! An application's clock source supplies elapsed time; its sole admission
//! owner consumes bounded tick proposals and dispatches domain/graph work.
use std::num::NonZeroUsize;
use std::time::Duration;
use thiserror::Error;

/// Bound the *number of tick dispatches proposed by one frame*, independently
/// of session-wide graph concurrency and any domain-internal worker count.
pub const MAX_FIXED_TICKS_PER_FRAME: usize = 1_024;
/// Bound deferred logical ticks; never accumulate unbounded catch-up debt.
pub const MAX_FIXED_TICK_BACKLOG: usize = 65_536;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedStepBacklog {
    /// Discard excessive logical ticks explicitly, preserving fractional time.
    /// Suitable for interactive simulations that must not spiral indefinitely.
    DiscardExcess,
    /// Retain at most this many ticks *in addition* to the frame's dispatch
    /// limit. Excess ticks are still explicitly reported as discarded.
    RetainUpTo(NonZeroUsize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FixedStepError {
    #[error("a fixed-step interval must be strictly positive")]
    ZeroInterval,
    #[error("ticks per frame exceeds the supported maximum of {maximum}")]
    FrameLimitExceeded { maximum: usize },
    #[error("retained tick backlog exceeds the supported maximum of {maximum}")]
    BacklogLimitExceeded { maximum: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedStepPolicy {
    interval: Duration,
    max_per_frame: NonZeroUsize,
    backlog: FixedStepBacklog,
}

impl FixedStepPolicy {
    /// Policy only; no implicit clock source, executor or application session.
    pub fn new(
        interval: Duration,
        max_per_frame: NonZeroUsize,
        backlog: FixedStepBacklog,
    ) -> Result<Self, FixedStepError> {
        if interval.is_zero() {
            return Err(FixedStepError::ZeroInterval);
        }
        if max_per_frame.get() > MAX_FIXED_TICKS_PER_FRAME {
            return Err(FixedStepError::FrameLimitExceeded {
                maximum: MAX_FIXED_TICKS_PER_FRAME,
            });
        }
        if let FixedStepBacklog::RetainUpTo(limit) = backlog
            && limit.get() > MAX_FIXED_TICK_BACKLOG
        {
            return Err(FixedStepError::BacklogLimitExceeded {
                maximum: MAX_FIXED_TICK_BACKLOG,
            });
        }
        Ok(Self {
            interval,
            max_per_frame,
            backlog,
        })
    }

    pub fn interval(&self) -> Duration {
        self.interval
    }

    pub fn max_per_frame(&self) -> NonZeroUsize {
        self.max_per_frame
    }

    pub fn backlog(&self) -> FixedStepBacklog {
        self.backlog
    }
}

/// Read-only outcome of accounting for elapsed time. These are *proposals*,
/// never confirmations that Rust, ECS or external effects have executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedStepFrame {
    /// Number of pending logical ticks that may be claimed in this frame.
    pub eligible: usize,
    /// Pending logical ticks deferred to a later frame by this policy.
    pub deferred: usize,
    /// Ticks discarded for exceeding the policy's bounded backlog.
    /// u128 avoids saturating when a huge elapsed time is supplied.
    pub discarded: u128,
    /// Fractional elapsed time carried toward the next complete interval.
    pub fractional: Duration,
}

/// Bounded logical-time accounting kept by exactly one chosen outer tick
/// source or a domain's own frame coordinator. No hidden timer is created.
///
/// The caller must provide non-overlapping *monotonic elapsed durations*.
/// `claim` transfers a proposal to the caller's dispatch responsibility;
/// it does not imply the callback ran or a remote side effect committed.
/// A new application session must create a fresh instance: this object is
/// not a session credential or replay/rollback mechanism.
#[derive(Debug)]
pub struct FixedStepAdmission {
    policy: FixedStepPolicy,
    fractional_ns: u128,
    pending: usize,
    frame_remaining: usize,
}

impl FixedStepAdmission {
    pub fn new(policy: FixedStepPolicy) -> Self {
        Self {
            policy,
            fractional_ns: 0,
            pending: 0,
            frame_remaining: 0,
        }
    }

    pub fn policy(&self) -> FixedStepPolicy {
        self.policy
    }

    /// Account for a single clock-owner frame. Large stalls are arithmetic,
    /// never loops proportional to elapsed time. Even if no work is claimed,
    /// every call starts a new bounded frame dispatch budget.
    pub fn advance_frame(&mut self, elapsed: Duration) -> FixedStepFrame {
        let interval_ns = self.policy.interval.as_nanos();
        // Duration cannot exceed ~2^94 nanoseconds, and fractional_ns is
        // always below one Duration interval; u128 addition cannot overflow.
        let elapsed_ns = self.fractional_ns + elapsed.as_nanos();
        let due = elapsed_ns / interval_ns + self.pending as u128;
        self.fractional_ns = elapsed_ns % interval_ns;
        let max_per_frame = self.policy.max_per_frame.get();
        let backlog = match self.policy.backlog {
            FixedStepBacklog::DiscardExcess => 0,
            FixedStepBacklog::RetainUpTo(limit) => limit.get(),
        };
        let bounded_pending = max_per_frame + backlog;
        let kept = due.min(bounded_pending as u128);
        self.pending = kept as usize;
        self.frame_remaining = max_per_frame;
        let eligible = self.pending.min(self.frame_remaining);
        FixedStepFrame {
            eligible,
            deferred: self.pending - eligible,
            discarded: due - kept,
            fractional: Duration::new(
                (self.fractional_ns / 1_000_000_000) as u64,
                (self.fractional_ns % 1_000_000_000) as u32,
            ),
        }
    }

    pub fn available(&self) -> usize {
        self.pending.min(self.frame_remaining)
    }

    /// Claim at most `requested` tick proposals in this frame. Unclaimed
    /// work stays pending until a subsequent frame policy decision; *claimed*
    /// work belongs to the caller, not to this accounting object. Do not
    /// automatically retry a claimed tick after uncertain external effects.
    pub fn claim(&mut self, requested: usize) -> usize {
        let amount = requested.min(self.available());
        self.pending -= amount;
        self.frame_remaining -= amount;
        amount
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(max: usize, backlog: FixedStepBacklog) -> FixedStepPolicy {
        FixedStepPolicy::new(
            Duration::from_millis(10),
            NonZeroUsize::new(max).unwrap(),
            backlog,
        )
        .unwrap()
    }

    #[test]
    fn rejects_unbounded_or_zero_policies_without_runtime_side_effects() {
        assert_eq!(
            FixedStepPolicy::new(
                Duration::ZERO,
                NonZeroUsize::new(1).unwrap(),
                FixedStepBacklog::DiscardExcess,
            ),
            Err(FixedStepError::ZeroInterval)
        );
        assert_eq!(
            FixedStepPolicy::new(
                Duration::from_nanos(1),
                NonZeroUsize::new(MAX_FIXED_TICKS_PER_FRAME + 1).unwrap(),
                FixedStepBacklog::DiscardExcess,
            ),
            Err(FixedStepError::FrameLimitExceeded {
                maximum: MAX_FIXED_TICKS_PER_FRAME,
            })
        );
        assert_eq!(
            FixedStepPolicy::new(
                Duration::from_millis(10),
                NonZeroUsize::new(1).unwrap(),
                FixedStepBacklog::RetainUpTo(
                    NonZeroUsize::new(MAX_FIXED_TICK_BACKLOG + 1).unwrap(),
                ),
            ),
            Err(FixedStepError::BacklogLimitExceeded {
                maximum: MAX_FIXED_TICK_BACKLOG,
            })
        );
    }

    #[test]
    fn exact_intervals_and_zero_elapsed_keep_fractional_time_without_double_ticks() {
        let mut policy = FixedStepAdmission::new(policy(2, FixedStepBacklog::DiscardExcess));
        assert_eq!(policy.advance_frame(Duration::ZERO).eligible, 0);
        assert_eq!(policy.claim(2), 0);
        let half = policy.advance_frame(Duration::from_millis(5));
        assert_eq!(half.eligible, 0);
        assert_eq!(half.fractional, Duration::from_millis(5));
        let whole = policy.advance_frame(Duration::from_millis(5));
        assert_eq!(whole.eligible, 1);
        assert_eq!(whole.fractional, Duration::ZERO);
        assert_eq!(policy.claim(1), 1);
        assert_eq!(policy.claim(1), 0);
        assert_eq!(policy.advance_frame(Duration::ZERO).eligible, 0);
        assert_eq!(
            policy.advance_frame(Duration::from_millis(1)).fractional,
            Duration::from_millis(1)
        );
    }

    #[test]
    fn interactive_drop_policy_reports_overrun_and_does_not_generate_hidden_backlog() {
        let mut policy = FixedStepAdmission::new(policy(2, FixedStepBacklog::DiscardExcess));
        let frame = policy.advance_frame(Duration::from_millis(45));
        assert_eq!(frame.eligible, 2);
        assert_eq!(frame.deferred, 0);
        assert_eq!(frame.discarded, 2);
        assert_eq!(frame.fractional, Duration::from_millis(5));
        assert_eq!(policy.claim(1), 1);
        assert_eq!(policy.available(), 1);
        // Unclaimed dispatch responsibility is not silently treated as done.
        let next = policy.advance_frame(Duration::from_millis(5));
        assert_eq!(next.eligible, 2);
        assert_eq!(next.discarded, 0);
        assert_eq!(policy.claim(5), 2);
        assert_eq!(policy.advance_frame(Duration::ZERO).eligible, 0);
    }

    #[test]
    fn retained_backlog_is_bounded_and_drained_only_after_claims() {
        let mut policy = FixedStepAdmission::new(policy(
            2,
            FixedStepBacklog::RetainUpTo(NonZeroUsize::new(3).unwrap()),
        ));
        let initial = policy.advance_frame(Duration::from_millis(85));
        assert_eq!(initial.eligible, 2);
        assert_eq!(initial.deferred, 3);
        assert_eq!(initial.discarded, 3);
        assert_eq!(initial.fractional, Duration::from_millis(5));
        assert_eq!(policy.claim(1), 1);
        // Next frame keeps the unclaimed tick plus bounded deferred debt.
        let next = policy.advance_frame(Duration::ZERO);
        assert_eq!(next.eligible, 2);
        assert_eq!(next.deferred, 2);
        assert_eq!(policy.claim(2), 2);
        assert_eq!(policy.advance_frame(Duration::ZERO).eligible, 2);
        assert_eq!(policy.claim(2), 2);
        assert_eq!(policy.advance_frame(Duration::ZERO).eligible, 0);
    }

    #[test]
    fn delayed_frames_cannot_overflow_u128_or_force_unbounded_catch_up() {
        let mut policy = FixedStepAdmission::new(
            FixedStepPolicy::new(
                Duration::from_nanos(1),
                NonZeroUsize::new(1).unwrap(),
                FixedStepBacklog::DiscardExcess,
            )
            .unwrap(),
        );
        let frame = policy.advance_frame(Duration::from_secs(u64::MAX));
        assert_eq!(frame.eligible, 1);
        assert_eq!(
            frame.discarded,
            Duration::from_secs(u64::MAX).as_nanos() - 1
        );
        assert_eq!(policy.claim(usize::MAX), 1);
        assert_eq!(policy.advance_frame(Duration::ZERO).eligible, 0);
    }

    #[test]
    fn fresh_instance_never_inherits_another_sessions_logical_tick_debt() {
        let config = policy(
            2,
            FixedStepBacklog::RetainUpTo(NonZeroUsize::new(5).unwrap()),
        );
        let mut previous = FixedStepAdmission::new(config);
        previous.advance_frame(Duration::from_millis(70));
        assert_eq!(previous.claim(1), 1);
        let mut restarted = FixedStepAdmission::new(config);
        assert_eq!(restarted.advance_frame(Duration::ZERO).eligible, 0);
        assert_eq!(restarted.claim(1), 0);
        assert!(previous.available() > 0);
    }
}
