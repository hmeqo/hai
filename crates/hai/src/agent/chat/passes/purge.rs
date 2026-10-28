//! The purge pass: its start, its success/failure completion and the host copy it delivers.

use uuid::Uuid;

use super::{super::ConversationAgent, BusyRun, BusySignal};
use crate::{
    agent::command::purge,
    config::conversation::Language,
    domain::vo::{AgentEvent, AgentEventPayload, ChatId, PurgeSpec},
    error::Result,
};

impl ConversationAgent {
    /// One purge: not a Turn and not a role, a background task of its own that owns the Conversation
    /// while it runs — an arrival rings the doorbell and waits its turn. The change that asked for it is
    /// consumed by the pass itself, failure included, so a broken purge is not read again at once;
    /// nothing else in the batch is spent.
    pub(crate) fn start_purge(&mut self, spec: PurgeSpec, entry_id: Uuid, origin: ChatId) {
        if self.chats.contains(origin) {
            self.chats.deactivate();
            self.chats.remember(origin);
        }
        let chat_id = self.chats.origin();
        let tool = self
            .chats
            .surface(chat_id)
            .expect("the origin is always a member")
            .tool()
            .clone();
        let events = self.engine.runtime.event_bus().clone();
        let shutdown = self.engine.shutdown();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let signal = tokio::select! {
                biased;
                _ = shutdown.cancelled() => BusySignal::Cancelled,
                report = purge::run(&tool, &events, spec, entry_id) => BusySignal::Purged(report),
            };
            let _ = tx.send(signal);
        });
        self.pass.begin(BusyRun::purge(rx, entry_id));
    }

    /// A purge ends: the changes it took leave the batch, a purged agent surface starts a fresh
    /// chapter, and the user hears what happened — as host copy, never as a stored message.
    pub(crate) async fn on_purge_done(
        &mut self,
        entry: Option<Uuid>,
        result: Result<purge::PurgeReport, String>,
    ) {
        match result {
            Ok(report) => {
                self.pacing.clear_purge();
                self.inbox.consume(&report.consumed);
                if report.spec.surfaces.agent {
                    // The value the transaction wrote: an empty chapter and its reset counters.
                    self.conversation.start_new_chapter(None);
                }
                let chat_id = self.chats.origin();
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
                self.pacing.defer_purge();
                let consumed: Vec<Uuid> = entry.into_iter().collect();
                if !consumed.is_empty() {
                    let snapshot = self.conversation.snapshot();
                    match self.persist_conversation(&snapshot, &consumed).await {
                        Ok(()) => self.inbox.consume(&consumed),
                        Err(e) => {
                            tracing::warn!(%self.conversation_id, "Failed to persist conversation: {e}");
                            self.stand_down();
                            return;
                        }
                    }
                }
                let language = self.receipt_language().await;
                self.notify(purge::copy::failed(language, &error)).await;
            }
        }
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
        let chat_id = self.chats.origin();
        let Ok(chat) = self.chats.surface(chat_id) else {
            return;
        };
        if let Err(e) = chat.tool().chat.notice(&text).await {
            tracing::warn!(%self.conversation_id, "host copy not delivered: {e}");
        }
    }
}
