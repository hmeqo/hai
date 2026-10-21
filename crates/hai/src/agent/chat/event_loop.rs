use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use uuid::Uuid;

use super::{
    super::events::{Doorbell, WakeEvents},
    BusyPass, ChatAgent, ChatAgentState, RETRY_BACKOFF,
    handle::ChatAgentStatus,
    scheduler::Decision,
};
use crate::{
    agent::{
        chat::types::{BusySignal, DigestRequest},
        command::purge,
    },
    domain::vo::{AgentEvent, AgentEventPayload, PurgeSpec, SessionRef, StartedBy},
    error::Result,
};

enum NextStep {
    Activate(StartedBy),
    WrapUp,
    /// A confirmed purge waits for its own pass, which runs before the scheduler is consulted.
    Purge {
        spec: PurgeSpec,
        entry_id: Uuid,
    },
    /// A digest somebody confirmed, waiting for the Busy slot.
    Digest(DigestRequest),
    Idle,
    Exit,
}

impl ChatAgent {
    pub(crate) async fn run(
        &mut self,
        doorbell: Doorbell,
        mut status_rx: mpsc::UnboundedReceiver<oneshot::Sender<ChatAgentStatus>>,
    ) {
        let shutdown = self.shutdown.clone();
        loop {
            match self.state.take() {
                ChatAgentState::Idle => {
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => break,
                        next = self.idle_tick(&doorbell, &mut status_rx) => match next {
                            NextStep::Activate(started_by) => {
                                self.dispatch(&doorbell, started_by).await
                            }
                            NextStep::Idle => self.state = ChatAgentState::Idle,
                            NextStep::WrapUp => self.start_wrap_up(&doorbell).await,
                            NextStep::Purge { spec, entry_id } => {
                                self.start_purge(spec, entry_id)
                            }
                            // A digest that never started leaves the chat Idle: the reply it was
                            // handed says so, and the page offers a retry.
                            NextStep::Digest(request) => self.start_digest(request).await,
                            NextStep::Exit => break,
                        },
                    }
                }
                ChatAgentState::Busy {
                    mut result_rx,
                    started_at,
                    pass,
                } => {
                    let signal = tokio::select! {
                        biased;
                        Some(query) = status_rx.recv() => {
                            self.state = ChatAgentState::busy(result_rx, started_at, pass);
                            self.answer_status(query);
                            continue;
                        }
                        result = &mut result_rx => result,
                    };
                    match signal {
                        Ok(BusySignal::Turn(output)) => {
                            self.on_complete(output).await;
                        }
                        Ok(BusySignal::Steered(output)) => {
                            self.on_complete(output).await;
                            // The turn just ended already spent the batch's budget, so this failure
                            // only ends the tick.
                            if let Err(e) = self.refresh_batch().await {
                                tracing::warn!(%self.chat_id, "refresh after steering failed: {e}");
                                self.end_run();
                                continue;
                            }
                            if !self.batch.is_empty() {
                                self.dispatch(&doorbell, StartedBy::Steered).await;
                            }
                        }
                        Ok(BusySignal::WrapUp {
                            summary,
                            context_tokens,
                            session,
                        }) => {
                            if self
                                .on_wrap_up_done(summary, context_tokens, session)
                                .await
                                .is_err()
                            {
                                self.stand_down();
                            }
                            self.end_run();
                        }
                        Ok(BusySignal::Cancelled) => self.end_run(),
                        Ok(BusySignal::Purged(result)) => self.on_purge_done(result).await,
                        Ok(BusySignal::Digested(result)) => self.on_digest_done(result),
                        Ok(BusySignal::TurnFailed) => {
                            tracing::warn!(%self.chat_id, "Busy task failed");
                            if self.retry.fail_now() {
                                tracing::warn!(
                                    %self.chat_id,
                                    remaining = self.retry.remaining(),
                                    "Re-dispatching after a failed turn"
                                );
                            }
                            self.end_run();
                        }
                        Ok(BusySignal::WrapUpFailed { error, session }) => {
                            self.engine.runtime.app.event_bus.emit(AgentEvent {
                                chat_id: self.chat_id,
                                session: Some(session),
                                payload: AgentEventPayload::WrapUpFailed { error },
                            });
                            self.conversation.start_new_chapter(None);
                            if self.persist_conversation(&[]).await.is_err() {
                                self.stand_down();
                            }
                            self.end_run();
                        }
                        Err(_) => {
                            tracing::warn!(%self.chat_id, "Busy task panicked");
                            self.end_run();
                        }
                    }
                }
            }
        }
    }

    fn should_wrap_up_by_tokens(&self) -> bool {
        let threshold = self
            .engine
            .runtime
            .app
            .cfg
            .agent
            .context
            .compact_token_threshold;
        threshold > 0 && self.conversation.context_tokens() >= threshold
    }

    async fn start_wrap_up(&mut self, doorbell: &Doorbell) {
        let language = match self.language().await {
            Ok(language) => language,
            Err(e) => {
                tracing::warn!(%self.chat_id, "language lookup failed: {e}");
                self.wrap_up_retry_at = Some(tokio::time::Instant::now() + RETRY_BACKOFF);
                return;
            }
        };
        self.wrap_up_retry_at = None;
        let ctx = self.build_turn_context(WakeEvents::new(vec![]), language);
        let result_rx = self.engine.spawn_wrap_up(
            ctx,
            self.tools.clone(),
            self.conversation.messages_for_wrap_up(),
            doorbell.clone(),
            self.conversation.session(),
        );
        self.state = ChatAgentState::busy(result_rx, tokio::time::Instant::now(), BusyPass::WrapUp);
    }

    async fn idle_tick(
        &mut self,
        doorbell: &Doorbell,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<ChatAgentStatus>>,
    ) -> NextStep {
        if let Some(at) = self.retry.owed_at() {
            if self.wait_until(at, status_rx).await {
                return NextStep::Idle;
            }
            self.retry.dispatched();
            return NextStep::Activate(StartedBy::Retry);
        }
        if let Some(at) = self.wrap_up_retry_at
            && self.wait_until(at, status_rx).await
        {
            return NextStep::Idle;
        }
        if self.should_wrap_up_by_tokens() {
            return NextStep::WrapUp;
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
        let deadline = self.schedule.next_deadline(timeout);

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
            tracing::warn!(%self.chat_id, "refresh_batch failed: {e}");
            self.stand_down();
            return NextStep::Idle;
        }

        // A pending purge goes before everything: what it deletes may be exactly what a wrap-up would
        // summarize, and the scheduler must not pick its moment. A purge whose files or statements
        // failed waits out the backoff instead of being retried on every tick, and nothing else runs
        // while it waits.
        if let Some((entry_id, spec)) = super::pending_purge(&self.batch) {
            if let Some(at) = self.purge_retry_at
                && self.wait_until(at, status_rx).await
            {
                return NextStep::Idle;
            }
            return NextStep::Purge { spec, entry_id };
        }

        let has_pending = !self.batch.is_empty() && self.retry.remaining() > 0;
        match self.schedule.decide(has_pending, timeout) {
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
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<ChatAgentStatus>>,
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

    async fn on_wrap_up_done(
        &mut self,
        summary: String,
        context_tokens: u32,
        session: SessionRef,
    ) -> Result<()> {
        let wrapped_up_steps = self.conversation.step_count();
        self.engine.runtime.event_bus().emit(AgentEvent {
            chat_id: self.chat_id,
            session: Some(session),
            payload: AgentEventPayload::WrapUpCompleted {
                step_count: wrapped_up_steps as usize,
                context_tokens,
                summary: summary.clone(),
            },
        });
        self.conversation.start_new_chapter(Some(summary));

        self.persist_conversation(&[]).await
    }

    /// A purge ends: the changes it took leave the batch, a purged agent surface starts a fresh
    /// chapter, and the user hears what happened — as host copy, never as a stored message.
    async fn on_purge_done(&mut self, result: Result<purge::PurgeReport, String>) {
        let language = match self.language().await {
            Ok(language) => language,
            Err(e) => {
                tracing::warn!(%self.chat_id, "language lookup failed: {e}");
                self.end_run();
                return;
            }
        };
        match result {
            Ok(report) => {
                self.purge_retry_at = None;
                self.purge_entry = None;
                self.batch
                    .retain(|entry| !report.consumed.contains(&entry.id));
                if report.spec.surfaces.agent {
                    // The value the transaction wrote: an empty chapter and its reset counters.
                    self.conversation.start_new_chapter(None);
                }
                self.engine.runtime.event_bus().emit(AgentEvent {
                    chat_id: self.chat_id,
                    session: None,
                    payload: AgentEventPayload::Purged {
                        spec: report.spec,
                        counts: report.counts,
                        telegram_deleted: report.telegram.deleted,
                        telegram_failed: report.telegram.failed.len() as u64,
                        files_removed: report.files_removed,
                        cached_removed: report.cached_removed,
                    },
                });
                // The platform's own reasons belong in the log, one line each; the receipt only says
                // how many could not be deleted.
                for failure in &report.telegram.failed {
                    tracing::warn!(
                        %self.chat_id,
                        external_id = %failure.external_id,
                        detail = %failure.detail,
                        "message not deleted",
                    );
                }
                self.notify(purge::copy::report(language, &report)).await;
            }
            Err(error) => {
                // The transaction rolled back, so nothing of the agent surface was deleted. Only the
                // purge's own change is spent — a purge that cannot run must not be read again on
                // the next tick — while everything else the batch holds was never read and stays for
                // the Turn that follows.
                self.purge_retry_at = Some(Instant::now() + RETRY_BACKOFF);
                let consumed: Vec<Uuid> = self.purge_entry.take().into_iter().collect();
                if !consumed.is_empty() {
                    match self.persist_conversation(&consumed).await {
                        Ok(()) => self.batch.retain(|entry| !consumed.contains(&entry.id)),
                        Err(e) => {
                            tracing::warn!(%self.chat_id, "Failed to persist conversation: {e}");
                            self.stand_down();
                            self.end_run();
                            return;
                        }
                    }
                }
                self.notify(purge::copy::failed(language, &error)).await;
            }
        }
        self.end_run();
    }

    /// Delivers host copy about a pass that just ended. A failure is logged and goes no further: the
    /// pass itself already happened, and copy that could not be delivered does not undo it.
    async fn notify(&self, text: String) {
        if let Err(e) = self.tool.chat.notice(&text).await {
            tracing::warn!(%self.chat_id, "host copy not delivered: {e}");
        }
    }

    pub fn build_status(&mut self) -> ChatAgentStatus {
        let turn_elapsed = match &self.state {
            ChatAgentState::Busy { started_at, .. } => Some(started_at.elapsed().as_secs_f64()),
            ChatAgentState::Idle => None,
        };
        let busy = match &self.state {
            ChatAgentState::Busy { pass, .. } => Some(*pass),
            ChatAgentState::Idle => None,
        };
        ChatAgentStatus {
            scheduler: self.schedule.snapshot(),
            step_count: self.conversation.step_count() as usize,
            context_tokens: self.conversation.context_tokens(),
            conversation_msgs: self.conversation.message_count(),
            turn_elapsed_secs: turn_elapsed,
            busy,
            model: self
                .engine
                .runtime
                .app
                .cfg
                .model_roles
                .default
                .model
                .clone(),
        }
    }

    pub fn answer_status(&mut self, query: oneshot::Sender<ChatAgentStatus>) {
        let _ = query.send(self.build_status());
    }
}
