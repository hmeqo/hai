//! 事件派发：dispatch + on_complete

use tokio::time::Instant;

use super::{
    super::{context::RunContext, event::WakeEvents},
    AgentSession, SessionState,
    conversation::RunInput,
};
use crate::{
    agent::runtime::{event::Inbox, types::RunOutput},
    domain::{
        model::Message,
        vo::{AgentEventPayload, MessageId},
    },
    error::Result,
};

// ── 生命周期 ───────────────────────────────────────────────────────────────

impl AgentSession {
    pub async fn dispatch(&mut self, events: WakeEvents, inbox: &Inbox) {
        let ctx = self.build_run_context(events);
        let (messages, next_since_id) = match self.gather_messages().await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(%self.chat_id, "gather_messages failed: {e}");
                self.state = SessionState::Idle;
                return;
            }
        };

        self.conversation.set_since_id(next_since_id);

        let built = match self
            .runtime
            .build_prompt(
                &ctx,
                &messages,
                self.conversation.shown_memory_ids(),
                self.conversation.shown_topic_ids(),
                self.run_count == 0,
            )
            .await
        {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(%self.chat_id, "build_prompt failed: {e}");
                self.state = SessionState::Idle;
                return;
            }
        };

        self.conversation
            .record_shown(&built.shown_memory_ids, &built.shown_topic_ids);
        let messages = self.conversation.build_full_messages(built.messages);
        let run_number = self.run_count + 1;

        let reason = ctx
            .events
            .first()
            .map(|e| e.reason.label())
            .unwrap_or("unknown")
            .to_string();

        self.engine.app.event_bus.emit(
            self.chat_id,
            AgentEventPayload::RunStarted {
                run: run_number,
                reason,
                msg_count: built.message_ids.len(),
                full_prompt: built.rendered_prompt.clone(),
            },
        );

        let (handle, result_rx) = self.runtime.spawn_run(
            ctx,
            RunInput {
                messages,
                message_ids: built.message_ids.iter().map(|id| MessageId(*id)).collect(),
                since_id: next_since_id,
            },
            inbox.clone(),
            run_number,
        );

        self.state = SessionState::Busy {
            handle,
            result_rx,
            started_at: Instant::now(),
        };
    }

    pub async fn on_complete(&mut self, output: RunOutput, inbox: &Inbox) {
        let has_spoken = output
            .turns
            .iter()
            .flat_map(|t| &t.tool_calls)
            .any(|tc| matches!(tc.tool_name.as_str(), "send_message" | "send_voice"));

        if has_spoken {
            self.schedule.refresh();
        }

        self.conversation.update(output.turns, output.messages);
        self.run_count += 1;

        let snap = self.conversation.snapshot();
        if let Err(e) = self
            .engine
            .app
            .db
            .srv
            .conversation
            .save(&snap, self.chat_id)
            .await
        {
            tracing::warn!(%self.chat_id, "Failed to persist conversation: {e}");
        }
        let events = inbox.drain();
        self.schedule.enqueue(events);
        self.state = SessionState::Idle;
    }

    pub fn build_run_context(&self, events: WakeEvents) -> RunContext {
        RunContext {
            app: self.engine.app.clone(),
            chat_id: self.chat_id,
            chat_type: self.chat_type,
            handler: self.runtime.handler.clone(),
            events,
            skill_manager: self.engine.skill_manager.clone(),
            db: self.engine.app.db.srv.clone(),
            shell: self.runtime.shell.clone(),
            multimodal: self.engine.app.provider.multimodal.clone(),
        }
    }

    pub async fn gather_messages(&self) -> Result<(Vec<Message>, MessageId)> {
        let srv = &self.engine.app.db.srv.message;
        let cfg = &self.engine.app.cfg.agent.context;

        if self.conversation.is_fresh() {
            let (msgs, last_id) = srv.get_context_messages(self.chat_id, 10).await?;
            Ok((msgs, MessageId(last_id)))
        } else {
            let since_id = self.conversation.since_id();
            let msgs = srv
                .get_messages_window(self.chat_id, Some(since_id), cfg.history_cap)
                .await?;
            let next_id = msgs.last().map(|m| m.id_()).unwrap_or(since_id);
            Ok((msgs, next_id))
        }
    }
}
