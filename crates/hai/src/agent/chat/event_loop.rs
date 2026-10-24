use tokio::{
    sync::{mpsc, oneshot},
    time::Instant,
};
use uuid::Uuid;

use super::{
    super::events::{Doorbell, WakeEvents, WakeReason},
    BusyPass, ConversationAgent, ConversationAgentState, RETRY_BACKOFF,
    handle::ConversationAgentStatus,
    scheduler::Decision,
};
use crate::{
    agent::{
        chat::types::{BusySignal, DigestRequest},
        command::purge,
    },
    config::conversation::Language,
    domain::{
        model::ChatType,
        vo::{
            AgentEvent, AgentEventPayload, ChatEntry, ChatId, EntryKind, PurgeSpec, SessionRef,
            StartedBy,
        },
    },
    error::Result,
};

fn batch_has_wake_reason(
    batch: &[ChatEntry],
    chat_type: impl Fn(ChatId) -> Option<ChatType>,
) -> bool {
    batch.iter().any(|entry| {
        chat_type(entry.chat_id)
            .is_some_and(|chat_type| WakeReason::of_entry(entry, chat_type).is_some())
    })
}

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
        let shutdown = self.shutdown.clone();
        loop {
            match self.state.take() {
                ConversationAgentState::Idle => {
                    tokio::select! {
                        biased;
                        _ = shutdown.cancelled() => break,
                        next = self.idle_tick(&doorbell, &mut status_rx) => match next {
                            NextStep::Activate(started_by) => {
                                self.dispatch(&doorbell, started_by).await
                            }
                            NextStep::Idle => self.state = ConversationAgentState::Idle,
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
                ConversationAgentState::Busy {
                    mut result_rx,
                    started_at,
                    pass,
                } => {
                    let signal = tokio::select! {
                        biased;
                        Some(query) = status_rx.recv() => {
                            self.state = ConversationAgentState::busy(result_rx, started_at, pass);
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
                            if let Err(e) = self.refresh_batch().await {
                                tracing::warn!(%self.conversation_id, "refresh after steering failed: {e}");
                                self.stand_down();
                                let _ = self
                                    .wait_until(Instant::now() + RETRY_BACKOFF, &mut status_rx)
                                    .await;
                                self.end_run();
                                continue;
                            }
                            if super::pending_purge(&self.batch).is_none()
                                && !self.force_wrap_up
                                && self.has_wake_reason()
                            {
                                self.dispatch(&doorbell, StartedBy::Steered).await;
                            }
                            continue;
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
                            tracing::warn!(%self.conversation_id, "Busy task failed");
                            if self.retry.fail_now() {
                                tracing::warn!(
                                    %self.conversation_id,
                                    remaining = self.retry.remaining(),
                                    "Re-dispatching after a failed turn"
                                );
                            }
                            self.end_run();
                        }
                        Ok(BusySignal::WrapUpFailed { error, session }) => {
                            let chat_id = self.origin();
                            self.engine.runtime.app.event_bus.emit(AgentEvent {
                                chat_id,
                                session: Some(session),
                                payload: AgentEventPayload::WrapUpFailed { error },
                            });
                            let consumed = std::mem::take(&mut self.wrap_up_consumed);
                            let snapshot = self.conversation.next_chapter_snapshot(None);
                            if self
                                .persist_conversation(&snapshot, &consumed)
                                .await
                                .is_ok()
                            {
                                self.conversation.apply_snapshot(snapshot);
                                self.force_wrap_up = false;
                                self.batch.retain(|entry| !consumed.contains(&entry.id));
                            } else {
                                self.stand_down();
                            }
                            self.end_run();
                        }
                        Err(_) => {
                            tracing::warn!(%self.conversation_id, "Busy task panicked");
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
        let settings = match self
            .engine
            .runtime
            .app
            .settings
            .snapshot(self.conversation_id)
            .await
        {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "settings lookup failed: {e}");
                self.wrap_up_retry_at = Some(tokio::time::Instant::now() + RETRY_BACKOFF);
                return;
            }
        };
        self.wrap_up_retry_at = None;
        self.wrap_up_consumed = if self.force_wrap_up {
            self.batch
                .iter()
                .filter(|entry| matches!(entry.kind, EntryKind::PolicyChanged))
                .map(|entry| entry.id)
                .collect()
        } else {
            Vec::new()
        };
        let chat_id = self.origin();
        let ctx = self.build_turn_context(
            chat_id,
            Vec::new(),
            WakeEvents::new(vec![]),
            settings.language,
            None,
            settings.content_policy,
        );
        let model = match self.selected_model(settings.model).await {
            Ok(model) => model,
            Err(e) => {
                tracing::warn!(%self.conversation_id, "model selection failed: {e}");
                self.wrap_up_retry_at = Some(tokio::time::Instant::now() + RETRY_BACKOFF);
                return;
            }
        };
        let tools = self
            .chat(chat_id)
            .expect("the chat was just resolved")
            .tools
            .clone();
        self.membership.deactivate();
        let result_rx = self.engine.spawn_wrap_up(
            ctx,
            tools,
            self.conversation.messages_for_wrap_up(),
            doorbell.clone(),
            self.conversation.session(),
            &model,
        );
        self.state =
            ConversationAgentState::busy(result_rx, tokio::time::Instant::now(), BusyPass::WrapUp);
    }

    /// A batch the scheduler may spend a Turn on. A host action is context, not a reason to
    /// dispatch, and the retry budget only matters when something can select an origin.
    fn has_wake_reason(&self) -> bool {
        self.retry.remaining() > 0
            && batch_has_wake_reason(&self.batch, |chat_id| {
                self.chats.get(&chat_id).map(|chat| chat.roster.chat_type)
            })
    }

    async fn idle_tick(
        &mut self,
        doorbell: &Doorbell,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<ConversationAgentStatus>>,
    ) -> NextStep {
        if let Some(at) = self.wrap_up_retry_at
            && self.wait_until(at, status_rx).await
        {
            return NextStep::Idle;
        }
        if let Some((snapshot, consumed)) = self.pending_persist.take() {
            match self.persist_conversation(&snapshot, &consumed).await {
                Ok(()) => {
                    self.conversation.apply_snapshot(snapshot);
                    self.batch.retain(|entry| !consumed.contains(&entry.id));
                    self.membership.clear_dirty();
                    self.retry.reopen();
                }
                Err(e) => {
                    tracing::warn!(%self.conversation_id, "pending snapshot still not persisted: {e}");
                    self.pending_persist = Some((snapshot, consumed));
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
        if (!self.batch.is_empty() || self.schedule.window_active())
            && let Err(e) = self.refresh_batch().await
        {
            tracing::warn!(%self.conversation_id, "refresh_batch failed: {e}");
            self.stand_down();
            let _ = self
                .wait_until(Instant::now() + RETRY_BACKOFF, status_rx)
                .await;
            return NextStep::Idle;
        }
        if let Some((entry_id, chat_id, spec)) = super::pending_purge(&self.batch) {
            if let Some(at) = self.purge_retry_at
                && self.wait_until(at, status_rx).await
            {
                return NextStep::Idle;
            }
            return NextStep::Purge {
                spec,
                entry_id,
                chat_id,
            };
        }
        if self.force_wrap_up || self.should_wrap_up_by_tokens() {
            return NextStep::WrapUp;
        }
        if let Some(at) = self.retry.owed_at() {
            if self.wait_until(at, status_rx).await {
                return NextStep::Idle;
            }
            self.retry.dispatched();
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
        let deadline = self.schedule.next_deadline(timeout, self.has_wake_reason());

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
        if let Some((entry_id, chat_id, spec)) = super::pending_purge(&self.batch) {
            if let Some(at) = self.purge_retry_at
                && self.wait_until(at, status_rx).await
            {
                return NextStep::Idle;
            }
            return NextStep::Purge {
                spec,
                entry_id,
                chat_id,
            };
        }
        if self.force_wrap_up || self.should_wrap_up_by_tokens() {
            return NextStep::WrapUp;
        }

        let has_pending = self.has_wake_reason();
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

    async fn on_wrap_up_done(
        &mut self,
        summary: String,
        context_tokens: u32,
        session: SessionRef,
    ) -> Result<()> {
        let wrapped_up_steps = self.conversation.step_count();
        let consumed = std::mem::take(&mut self.wrap_up_consumed);
        let snapshot = self
            .conversation
            .next_chapter_snapshot(Some(summary.clone()));
        self.persist_conversation(&snapshot, &consumed).await?;
        self.conversation.apply_snapshot(snapshot);
        self.force_wrap_up = false;
        self.batch.retain(|entry| !consumed.contains(&entry.id));
        let chat_id = self.origin();
        self.engine.runtime.event_bus().emit(AgentEvent {
            chat_id,
            session: Some(session),
            payload: AgentEventPayload::WrapUpCompleted {
                step_count: wrapped_up_steps as usize,
                context_tokens,
                summary,
            },
        });
        Ok(())
    }

    /// A purge ends: the changes it took leave the batch, a purged agent surface starts a fresh
    /// chapter, and the user hears what happened — as host copy, never as a stored message.
    async fn on_purge_done(&mut self, result: Result<purge::PurgeReport, String>) {
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
                let chat_id = self.origin();
                self.engine.runtime.event_bus().emit(AgentEvent {
                    chat_id,
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
                        %self.conversation_id,
                        external_id = %failure.external_id,
                        detail = %failure.detail,
                        "message not deleted",
                    );
                }
                let language = self.receipt_language().await;
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
                    let snapshot = self.conversation.snapshot();
                    match self.persist_conversation(&snapshot, &consumed).await {
                        Ok(()) => self.batch.retain(|entry| !consumed.contains(&entry.id)),
                        Err(e) => {
                            tracing::warn!(%self.conversation_id, "Failed to persist conversation: {e}");
                            self.stand_down();
                            self.end_run();
                            return;
                        }
                    }
                }
                let language = self.receipt_language().await;
                self.notify(purge::copy::failed(language, &error)).await;
            }
        }
        self.end_run();
    }

    /// The receipt's language never decides whether a committed purge is applied: a failed lookup
    /// logs and falls back to the configured default.
    async fn receipt_language(&self) -> Language {
        self.language().await.unwrap_or_else(|e| {
            tracing::warn!(%self.conversation_id, "language lookup failed: {e}");
            self.engine.runtime.cfg().conversation.language
        })
    }

    /// Delivers host copy about a pass that just ended. A failure is logged and goes no further: the
    /// pass itself already happened, and copy that could not be delivered does not undo it.
    async fn notify(&self, text: String) {
        let chat_id = self.origin();
        let Ok(chat) = self.chat(chat_id) else {
            return;
        };
        if let Err(e) = chat.tool.chat.notice(&text).await {
            tracing::warn!(%self.conversation_id, "host copy not delivered: {e}");
        }
    }

    pub fn build_status(&mut self) -> ConversationAgentStatus {
        let turn_elapsed = match &self.state {
            ConversationAgentState::Busy { started_at, .. } => {
                Some(started_at.elapsed().as_secs_f64())
            }
            ConversationAgentState::Idle => None,
        };
        let busy = match &self.state {
            ConversationAgentState::Busy { pass, .. } => Some(*pass),
            ConversationAgentState::Idle => None,
        };
        ConversationAgentStatus {
            scheduler: self.schedule.snapshot(),
            step_count: self.conversation.step_count() as usize,
            context_tokens: self.conversation.context_tokens(),
            conversation_msgs: self.conversation.message_count(),
            turn_elapsed_secs: turn_elapsed,
            busy,
            model: self.model_label.clone(),
        }
    }

    pub fn answer_status(&mut self, query: oneshot::Sender<ConversationAgentStatus>) {
        let _ = query.send(self.build_status());
    }
}

#[cfg(test)]
mod tests {
    use super::batch_has_wake_reason;
    use crate::domain::{
        model::ChatType,
        vo::{
            AgentCommand, ChatEntry, ChatId, EntryAudience, EntryKind, HostAction, PurgeSpec,
            PurgeSurfaces, Stretch,
        },
    };

    fn entry(kind: EntryKind, audience: EntryAudience) -> ChatEntry {
        ChatEntry::new(ChatId(1), audience, kind, jiff::Timestamp::now())
    }

    /// Context-only entries never make a batch dispatchable; a real wake reason does.
    #[test]
    fn a_host_action_alone_is_not_a_wake_reason() {
        let host = entry(
            EntryKind::HostAction(HostAction::PersonaChanged),
            EntryAudience::Context,
        );
        let message = entry(
            EntryKind::MessageCreated {
                message_id: crate::domain::vo::MessageId(1),
            },
            EntryAudience::Agent,
        );
        let purge = entry(
            EntryKind::Purge(PurgeSpec {
                range: Stretch::All,
                surfaces: PurgeSurfaces::all(),
            }),
            EntryAudience::Agent,
        );
        let command = entry(
            EntryKind::Command(AgentCommand::Explain(None)),
            EntryAudience::Agent,
        );

        let chat_type = |_| Some(ChatType::Group);
        assert!(!batch_has_wake_reason(
            std::slice::from_ref(&host),
            chat_type
        ));
        assert!(batch_has_wake_reason(&[message], chat_type));
        assert!(!batch_has_wake_reason(&[purge], chat_type));
        assert!(batch_has_wake_reason(&[command], chat_type));
        assert!(batch_has_wake_reason(
            &[
                host,
                entry(
                    EntryKind::MessageCreated {
                        message_id: crate::domain::vo::MessageId(2),
                    },
                    EntryAudience::Agent,
                ),
            ],
            chat_type
        ));
    }
}
