use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use uuid::Uuid;

use super::{
    super::events::Doorbell,
    ConversationAgent,
    handle::ConversationAgentStatus,
    pacing::RETRY_BACKOFF,
    passes::{BusyRun, BusySignal, DigestRequest, PassSlot},
    scheduler::Decision,
};
use crate::{
    domain::vo::{ChatId, PurgeSpec, StartedBy},
    error::{AppError, Result},
};

enum NextStep {
    Activate(StartedBy),
    WrapUp,
    /// A confirmed purge waits for its own pass, which runs before the scheduler is consulted.
    Purge {
        spec: PurgeSpec,
        entry_id: Uuid,
        chat_id: ChatId,
    },
    /// A digest somebody confirmed, waiting for the Busy slot.
    Digest(DigestRequest),
    Idle,
    Exit,
}

impl ConversationAgent {
    pub(crate) async fn run(
        &mut self,
        doorbell: Doorbell,
        mut status_rx: mpsc::UnboundedReceiver<oneshot::Sender<ConversationAgentStatus>>,
    ) {
        let shutdown = self.engine.shutdown();
        loop {
            match self.pass.take() {
                PassSlot::Idle => {
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => break,
                        next = self.idle_tick(&doorbell, &mut status_rx) => match next {
                            NextStep::Activate(started_by) => {
                                self.dispatch(&doorbell, started_by).await
                            }
                            NextStep::Idle => {}
                            NextStep::WrapUp => self.start_wrap_up(&doorbell).await,
                            NextStep::Purge {
                                spec,
                                entry_id,
                                chat_id,
                            } => self.start_purge(spec, entry_id, chat_id),
                            // A digest that never started leaves the chat Idle: the reply it was
                            // handed says so, and the page offers a retry.
                            NextStep::Digest(request) => self.start_digest(request).await,
                            NextStep::Exit => break,
                        },
                    }
                }
                PassSlot::Busy(mut busy) => {
                    let signal = tokio::select! {
                        biased;
                        Some(query) = status_rx.recv() => {
                            self.pass.begin(busy);
                            self.answer_status(query);
                            continue;
                        }
                        result = &mut busy.result_rx => result,
                    };
                    self.finish(busy, signal, &doorbell, &mut status_rx).await;
                }
            }
        }
    }

    /// The one Busy → Idle exit. Each signal hands its pass-owned values over exactly once, then
    /// `end_run` restarts the idle clock.
    async fn finish(
        &mut self,
        mut busy: BusyRun,
        signal: Result<BusySignal, oneshot::error::RecvError>,
        doorbell: &Doorbell,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<ConversationAgentStatus>>,
    ) {
        match signal {
            Ok(BusySignal::Turn(output)) => {
                self.on_complete(output).await;
            }
            Ok(BusySignal::Steered(output)) => {
                self.on_complete(output).await;
                self.end_run();
                if let Err(e) = self.refresh_batch().await {
                    tracing::warn!(%self.conversation_id, "refresh after steering failed: {e}");
                    self.stand_down();
                    let _ = self
                        .wait_until(Instant::now() + RETRY_BACKOFF, status_rx)
                        .await;
                    return;
                }
                if self.inbox.pending_purge().is_none()
                    && !self.inbox.policy_due()
                    && self.has_wake_reason()
                {
                    self.dispatch(doorbell, StartedBy::Steered).await;
                }
                return;
            }
            Ok(BusySignal::WrapUp {
                summary,
                context_tokens,
                session,
            }) => {
                let consumed = busy.take_wrap_up_consumed();
                if self
                    .on_wrap_up_done(summary, context_tokens, session, consumed)
                    .await
                    .is_err()
                {
                    self.stand_down();
                }
            }
            Ok(BusySignal::Cancelled) => {}
            Ok(BusySignal::Purged(result)) => {
                self.on_purge_done(busy.take_purge_entry(), result).await;
            }
            Ok(BusySignal::Digested(result)) => {
                self.on_digest_done(busy.take_digest_reply(), result);
            }
            Ok(BusySignal::TurnFailed) => {
                tracing::warn!(%self.conversation_id, "Busy task failed");
                if self.pacing.fail_turn() {
                    tracing::warn!(
                        %self.conversation_id,
                        remaining = self.pacing.remaining(),
                        "Re-dispatching after a failed turn"
                    );
                }
            }
            Ok(BusySignal::WrapUpFailed { error, session }) => {
                self.on_wrap_up_failed(busy.take_wrap_up_consumed(), error, session)
                    .await;
            }
            Err(_) => {
                tracing::warn!(%self.conversation_id, "Busy task panicked");
            }
        }
        self.end_run();
    }

    /// Leaves Busy: the idle clock restarts here.
    fn end_run(&mut self) {
        self.pacing.mark_run_end();
        self.chats.deactivate();
        self.pass.reset();
    }

    /// A failure before the Turn started: an owed attempt waits out the backoff instead of going at
    /// once. A spent budget leaves the batch waiting for a new change (`docs/topics/chat-agent.md`).
    pub(crate) fn stand_down(&mut self) {
        if self.pacing.fail_before_turn() {
            tracing::warn!(%self.conversation_id, "Re-dispatching after a failure before the turn started");
        } else {
            tracing::warn!(%self.conversation_id, "Retry budget spent; the batch waits for a new change");
        }
        self.chats.deactivate();
        self.pass.reset();
    }

    /// Re-reads membership and the accumulation, feeds the scheduler the entries it has not seen
    /// yet, and reopens the retry budget when anything is new.
    async fn refresh_batch(&mut self) -> Result<()> {
        self.chats.sync(&self.engine.runtime).await?;
        let entries = self
            .engine
            .runtime
            .app
            .services
            .conversation
            .pending_entries(self.conversation_id)
            .await?;
        if let Some(entry) = entries
            .iter()
            .find(|entry| !self.chats.contains(entry.chat_id))
        {
            return Err(AppError::message(format!(
                "pending entry {} names unknown Chat {}",
                entry.id, entry.chat_id
            )));
        }

        let refresh = self.inbox.replace(entries);
        if refresh.reopened {
            self.pacing.reopen();
        }
        for audience in refresh.audiences {
            self.pacing.on_event(audience);
        }
        Ok(())
    }

    fn has_wake_reason(&self) -> bool {
        self.pacing.remaining() > 0 && self.inbox.has_wake_reason(self.chats.roster_ref())
    }

    async fn idle_tick(
        &mut self,
        doorbell: &Doorbell,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<ConversationAgentStatus>>,
    ) -> NextStep {
        if let Some(at) = self.pacing.wrap_up_due()
            && self.wait_until(at, status_rx).await
        {
            return NextStep::Idle;
        }
        if let Some(write) = self.pending_write.take() {
            match self
                .persist_conversation(&write.snapshot, &write.consumed)
                .await
            {
                Ok(()) => {
                    self.conversation.apply_snapshot(write.snapshot);
                    self.inbox.consume(&write.consumed);
                    self.chats.clear_dirty();
                    self.pacing.reopen();
                }
                Err(e) => {
                    tracing::warn!(%self.conversation_id, "pending snapshot still not persisted: {e}");
                    self.pending_write = Some(write);
                    self.stand_down();
                    let _ = self
                        .wait_until(Instant::now() + RETRY_BACKOFF, status_rx)
                        .await;
                    return NextStep::Idle;
                }
            }
        }
        // Settle what is already in hand before choosing a sleep deadline: a batch left by a
        // previous Turn, or an open attention window, is work the scheduler must see now.
        if (!self.inbox.is_empty() || self.pacing.window_active())
            && let Err(e) = self.refresh_batch().await
        {
            tracing::warn!(%self.conversation_id, "refresh_batch failed: {e}");
            self.stand_down();
            let _ = self
                .wait_until(Instant::now() + RETRY_BACKOFF, status_rx)
                .await;
            return NextStep::Idle;
        }
        if let Some(change) = self.inbox.pending_purge() {
            if let Some(at) = self.pacing.purge_due()
                && self.wait_until(at, status_rx).await
            {
                return NextStep::Idle;
            }
            return NextStep::Purge {
                spec: change.spec,
                entry_id: change.entry_id,
                chat_id: change.chat_id,
            };
        }
        if self.inbox.policy_due() || self.should_wrap_up_by_tokens() {
            return NextStep::WrapUp;
        }
        if let Some(at) = self.pacing.retry_due() {
            if self.wait_until(at, status_rx).await {
                return NextStep::Idle;
            }
            self.pacing.dispatched();
            return NextStep::Activate(StartedBy::Retry);
        }
        if let Some(last_active) = self.conversation.take_restored_last_active()
            && self.conversation.has_unwrapped_content()
        {
            let timeout = jiff::SignedDuration::from_secs(self.idle_timeout().as_secs() as i64);
            if jiff::Timestamp::now().duration_since(last_active) > timeout {
                return NextStep::WrapUp;
            }
        }
        let timeout = self.idle_timeout();
        let deadline = self.pacing.next_deadline(timeout, self.has_wake_reason());

        tokio::select! {
            _ = doorbell.notified() => {},
            Some(query) = status_rx.recv() => {
                self.answer_status(query);
                return NextStep::Idle;
            }
            // A digest waits for the slot: it is work of the chat's own, and nothing else about the
            // chat may be running while it reads.
            Some(request) = self.digest_rx.recv() => return NextStep::Digest(request),
            _ = tokio::time::sleep_until(deadline) => {},
        }

        if let Err(e) = self.refresh_batch().await {
            tracing::warn!(%self.conversation_id, "refresh_batch failed: {e}");
            self.stand_down();
            return NextStep::Idle;
        }
        // A pending purge goes before everything: what it deletes may be exactly what a wrap-up would
        // summarize, and the scheduler must not pick its moment. A purge whose files or statements
        // failed waits out the backoff instead of being retried on every tick, and nothing else runs
        // while it waits.
        if let Some(change) = self.inbox.pending_purge() {
            if let Some(at) = self.pacing.purge_due()
                && self.wait_until(at, status_rx).await
            {
                return NextStep::Idle;
            }
            return NextStep::Purge {
                spec: change.spec,
                entry_id: change.entry_id,
                chat_id: change.chat_id,
            };
        }
        if self.inbox.policy_due() || self.should_wrap_up_by_tokens() {
            return NextStep::WrapUp;
        }

        let has_pending = self.has_wake_reason();
        match self.pacing.decide(has_pending, timeout) {
            Decision::Ready(started_by) => NextStep::Activate(started_by),
            Decision::Defer => NextStep::Idle,
            Decision::Done => {
                if self.conversation.has_unwrapped_content() {
                    NextStep::WrapUp
                } else {
                    NextStep::Exit
                }
            }
        }
    }

    /// Waits out a retry deadline inside the caller's `select!` branch, so shutdown still cancels it;
    /// a status query arriving during the wait is answered rather than left hanging. `true` means the
    /// tick must end.
    async fn wait_until(
        &mut self,
        at: Instant,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<ConversationAgentStatus>>,
    ) -> bool {
        tokio::select! {
            biased;
            Some(query) = status_rx.recv() => {
                self.answer_status(query);
                true
            }
            _ = tokio::time::sleep_until(at) => false,
        }
    }
}
