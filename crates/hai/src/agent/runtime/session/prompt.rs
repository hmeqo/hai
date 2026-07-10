//! 上下文构建：assemble_run + build_run_context + gather_messages + RunInput

use super::{
    super::{context::RunContext, event::WakeEvents},
    AgentSession,
};
use crate::{
    agent::runtime::types::Messages,
    domain::{model::Message, vo::MessageId},
    error::Result,
};

// ── RunInput ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub(super) struct RunInput {
    pub messages: Messages,
    pub prompt: String,
    pub message_ids: Vec<MessageId>,
    pub since_id: MessageId,
}

// ── 上下文构建 ─────────────────────────────────────────────────────────────

impl AgentSession {
    pub(super) fn build_run_context(&self, events: WakeEvents) -> RunContext {
        RunContext {
            app: self.engine.app.clone(),
            chat_id: self.chat_id,
            chat_type: self.chat_type,
            bot: self.bot.clone(),
            events,
            skill_manager: self.engine.skill_manager.clone(),
            db: self.engine.app.db.srv.clone(),
            shell: self.shell.clone(),
            multimodal: self.engine.app.provider.multimodal.clone(),
            enabled_parsers: self.enabled_parsers.clone(),
            tts_enabled: self.tts_enabled,
        }
    }

    pub(super) async fn gather_messages(&self) -> Result<(Vec<Message>, MessageId)> {
        let cfg = &self.engine.app.cfg.agent.context;
        let srv = &self.engine.app.db.srv.message;

        if self.conversation.run_count() == 0 {
            let (msgs, last_id) = srv
                .get_context_messages(self.chat_id, cfg.history_cap, 10)
                .await?;
            Ok((msgs, MessageId(last_id)))
        } else {
            let since_id = self.conversation.since_id;
            let msgs = srv
                .get_messages_window(self.chat_id, Some(since_id), cfg.history_cap)
                .await?;
            let next_id = msgs.last().map(|m| m.id_()).unwrap_or(since_id);
            Ok((msgs, next_id))
        }
    }

    pub(super) async fn assemble_run(
        &mut self,
        events: WakeEvents,
    ) -> Option<(RunContext, RunInput)> {
        let ctx = self.build_run_context(events);
        let (messages, next_since_id) = self.gather_messages().await.ok()?;

        let payload = self
            .conversation
            .next_prompt(&ctx, &messages, next_since_id)
            .await?;

        Some((ctx, payload))
    }
}
