//! When a ConversationAgent may spend a Turn, and the bounded retry/deadline state around it.

use tokio::time::{Duration, Instant};

use super::scheduler::{Decision, EventScheduler, SchedulerStatus};
use crate::{config::schema::AttentionConfig, domain::vo::EntryAudience};

/// Re-dispatches allowed per batch: three attempts in total, the first one included.
pub(super) const MAX_TURN_RETRIES: u8 = 2;

/// Wait before the attempt that follows a failure that is not the model's own turn.
pub(super) const RETRY_BACKOFF: Duration = Duration::from_secs(1);

struct RetryBudget {
    attempts_left: u8,
    /// When the owed attempt may go; `None` when nothing is owed. Reading it does not spend it — it
    /// is cleared where the attempt is dispatched, so a wait that is interrupted keeps the debt.
    owed_at: Option<Instant>,
}

impl RetryBudget {
    fn new() -> Self {
        Self {
            attempts_left: MAX_TURN_RETRIES,
            owed_at: None,
        }
    }

    /// A new reason to spend a turn leaves the budget whole.
    fn reopen(&mut self) {
        *self = Self::new();
    }

    /// A failed attempt: spends one attempt and returns whether the budget still owes another one,
    /// which may go at once — the model's own turn failed.
    fn fail_now(&mut self) -> bool {
        let owed = self.attempts_left > 0;
        self.attempts_left = self.attempts_left.saturating_sub(1);
        if owed {
            self.owed_at = Some(Instant::now());
        }
        owed
    }

    /// A failed attempt that waits out the backoff before the one it owes — the failure happened
    /// before the turn started.
    fn fail_after_backoff(&mut self) -> bool {
        let owed = self.fail_now();
        if owed {
            self.owed_at = Some(Instant::now() + RETRY_BACKOFF);
        }
        owed
    }

    fn remaining(&self) -> u8 {
        self.attempts_left
    }

    /// When the owed attempt may go.
    fn owed_at(&self) -> Option<Instant> {
        self.owed_at
    }

    /// The owed attempt went out; nothing is owed until another failure.
    fn dispatched(&mut self) {
        self.owed_at = None;
    }
}

/// The timing engine plus the bounded retry and wrap-up/purge backoffs. The scheduler itself is
/// pure; this owner adds the deadlines the run loop reads.
pub(super) struct Pacing {
    scheduler: EventScheduler,
    retry: RetryBudget,
    wrap_up_retry_at: Option<Instant>,
    purge_retry_at: Option<Instant>,
}

impl Pacing {
    pub(super) fn new(cfg: &AttentionConfig) -> Self {
        Self {
            scheduler: EventScheduler::new(cfg),
            retry: RetryBudget::new(),
            wrap_up_retry_at: None,
            purge_retry_at: None,
        }
    }

    pub(super) fn on_event(&mut self, audience: EntryAudience) {
        self.scheduler.on_event(audience);
    }

    pub(super) fn on_spoke(&mut self) {
        self.scheduler.on_spoke();
    }

    pub(super) fn window_active(&self) -> bool {
        self.scheduler.window_active()
    }

    pub(super) fn next_deadline(&self, idle_timeout: Duration, has_pending: bool) -> Instant {
        self.scheduler.next_deadline(idle_timeout, has_pending)
    }

    pub(super) fn decide(&mut self, has_pending: bool, idle_timeout: Duration) -> Decision {
        self.scheduler.decide(has_pending, idle_timeout)
    }

    pub(super) fn mark_run_end(&mut self) {
        self.scheduler.mark_run_end();
    }

    pub(super) fn snapshot(&mut self) -> SchedulerStatus {
        self.scheduler.snapshot()
    }

    pub(super) fn reopen(&mut self) {
        self.retry.reopen();
    }

    pub(super) fn fail_before_turn(&mut self) -> bool {
        self.retry.fail_after_backoff()
    }

    pub(super) fn fail_turn(&mut self) -> bool {
        self.retry.fail_now()
    }

    pub(super) fn remaining(&self) -> u8 {
        self.retry.remaining()
    }

    pub(super) fn retry_due(&self) -> Option<Instant> {
        self.retry.owed_at()
    }

    pub(super) fn dispatched(&mut self) {
        self.retry.dispatched();
    }

    pub(super) fn wrap_up_due(&self) -> Option<Instant> {
        self.wrap_up_retry_at
    }

    pub(super) fn defer_wrap_up(&mut self) {
        self.wrap_up_retry_at = Some(Instant::now() + RETRY_BACKOFF);
    }

    pub(super) fn clear_wrap_up(&mut self) {
        self.wrap_up_retry_at = None;
    }

    pub(super) fn purge_due(&self) -> Option<Instant> {
        self.purge_retry_at
    }

    pub(super) fn defer_purge(&mut self) {
        self.purge_retry_at = Some(Instant::now() + RETRY_BACKOFF);
    }

    pub(super) fn clear_purge(&mut self) {
        self.purge_retry_at = None;
    }
}

#[cfg(test)]
mod tests {
    use tokio::time::Instant;

    use super::{MAX_TURN_RETRIES, Pacing, RetryBudget};
    use crate::config::schema::AttentionConfig;

    fn cfg() -> AttentionConfig {
        AttentionConfig {
            base_attention: 0.0,
            window_secs: 30.0,
        }
    }

    #[test]
    fn a_model_failure_retries_at_once() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_now(), "the first failure still owes an attempt");
        assert!(retry.owed_at().is_some(), "the attempt is owed at once");
        retry.dispatched();
        assert!(retry.owed_at().is_none(), "a dispatched attempt settles it");
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES - 1);
    }

    #[test]
    fn a_failure_before_the_turn_waits_and_spends_the_budget() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_after_backoff());
        let at = retry.owed_at().expect("the owed attempt waits");
        assert!(at > Instant::now(), "it waits out the backoff");
        retry.dispatched();
        assert!(retry.owed_at().is_none());
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES - 1);
    }

    /// The debt survives being read: only a dispatched attempt settles it, so a wait that a status
    /// query interrupts keeps the retry owed.
    #[test]
    fn an_owed_attempt_survives_being_read() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_after_backoff());
        assert!(retry.owed_at().is_some());
        assert!(retry.owed_at().is_some(), "reading it does not spend it");
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES - 1);
    }

    #[test]
    fn a_failure_before_the_turn_stops_owing_once_the_budget_is_spent() {
        let mut retry = RetryBudget::new();
        assert!(retry.fail_after_backoff());
        assert!(retry.fail_after_backoff());
        assert!(!retry.fail_after_backoff(), "a spent budget owes nothing");
        assert_eq!(retry.remaining(), 0, "the budget never goes below zero");
    }

    #[test]
    fn a_new_reason_leaves_the_budget_whole() {
        let mut retry = RetryBudget::new();
        retry.fail_after_backoff();
        retry.fail_now();
        retry.reopen();
        assert_eq!(retry.remaining(), MAX_TURN_RETRIES);
        assert!(retry.owed_at().is_none());
    }

    #[test]
    fn a_failed_wrap_up_backs_off_until_cleared() {
        let mut pacing = Pacing::new(&cfg());
        assert!(pacing.wrap_up_due().is_none());
        pacing.defer_wrap_up();
        assert!(pacing.wrap_up_due().is_some());
        pacing.clear_wrap_up();
        assert!(pacing.wrap_up_due().is_none());
    }
}
