use tokio::time::Duration;

use super::{
    super::{ctx::RoundContext, event::WakeEvent, round::RoundTaskPayload},
    SessionLoop,
};
use crate::{
    agent::context,
    agentcore::render::{Format, render_pretty},
    config::schema::SessionConfig,
};

impl SessionLoop {
    pub(super) async fn assemble_round(
        &self,
        events: Vec<WakeEvent>,
    ) -> Option<(RoundContext, RoundTaskPayload)> {
        let ctx = self.build_round_context(events);
        let (messages, next_since_id) = self.gather_messages().await;

        let built = if self.rounds.last().is_some() {
            match context::build_next_round_prompt(&ctx, &messages).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(%self.chat_id, "build_next_round_prompt failed: {e}");
                    return None;
                }
            }
        } else {
            match context::build_first_round_prompt(&ctx, &messages).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(%self.chat_id, "build_first_round_prompt failed: {e}");
                    return None;
                }
            }
        };

        let mut segment = built.rendered_prompt;
        let is_continuous = self.engine.app.cfg.agent.context.session == SessionConfig::Continuous;

        if is_continuous
            && let Some(prev_round) = self.rounds.last()
            && let Some(round_end) = context::build_round_end_section(prev_round)
        {
            segment = format!(
                "{round_end_xml}\n{segment}",
                round_end_xml = render_pretty(round_end, Format::Xml)
            );
        }

        let prompt = if !self.rounds.is_empty() {
            format!("{}\n{segment}", self.full_prompt())
        } else {
            segment.clone()
        };

        Some((
            ctx,
            RoundTaskPayload {
                prompt,
                segment,
                message_ids: built.message_ids,
                since_id: next_since_id,
            },
        ))
    }

    fn full_prompt(&self) -> String {
        self.rounds
            .iter()
            .map(|r| r.segment.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn build_round_context(&self, events: Vec<WakeEvent>) -> RoundContext {
        let mut seen = std::collections::HashSet::new();
        let events: Vec<WakeEvent> = events
            .into_iter()
            .filter(|e| {
                if e.reason.is_mergeable() {
                    seen.insert(e.reason.label())
                } else {
                    true
                }
            })
            .collect();

        RoundContext {
            app: self.engine.app.clone(),
            chat_id: self.chat_id,
            chat_type: self.chat_type,
            bot: self.bot.clone(),
            events,
            skill_manager: self.engine.skill_manager.clone(),
            db: self.engine.app.db.srv.clone(),
            shell: self.shell.clone(),
            multimodal: self.engine.app.provider.multimodal.clone(),
            sandbox: self.engine.app.cfg.sandbox.clone(),
            enabled_parsers: self.enabled_parsers.clone(),
            tts_enabled: self.tts_enabled,
        }
    }

    pub(super) async fn gather_messages(&self) -> (Vec<crate::domain::entity::Message>, i64) {
        let cfg = &self.engine.app.cfg.agent.context;
        let message = &self.engine.app.db.srv.message;

        if self.rounds.is_empty() {
            let mut messages = match message
                .get_unread_messages(self.chat_id, cfg.history_cap)
                .await
            {
                Ok(msgs) => msgs,
                Err(e) => {
                    tracing::error!(%self.chat_id, "Failed to fetch unread messages: {e}");
                    return (Vec::new(), -1);
                }
            };

            if (messages.len() as i64) < cfg.message_history_limit {
                let need = cfg.message_history_limit as usize - messages.len();
                if let Ok(history) = message.get_read_messages(self.chat_id, need as i64).await {
                    let ids: std::collections::HashSet<_> =
                        messages.iter().map(|m| m.id).collect();
                    let mut padded: Vec<_> =
                        history.into_iter().filter(|m| !ids.contains(&m.id)).collect();
                    padded.extend(messages);
                    padded.sort_by_key(|m| m.id);
                    messages = padded;
                }
            }

            let next_id = messages.last().map(|m| m.id).unwrap_or(-1);
            (messages, next_id)
        } else {
            let since_id = self.rounds.last().unwrap().since_id;
            match message
                .get_messages_window(self.chat_id, Some(since_id), cfg.history_cap)
                .await
            {
                Ok(msgs) => {
                    let next_id = msgs.last().map(|m| m.id).unwrap_or(since_id);
                    (msgs, next_id)
                }
                Err(e) => {
                    tracing::error!(%self.chat_id, "Failed to gather messages: {e}");
                    (Vec::new(), since_id)
                }
            }
        }
    }

    pub(super) fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.engine.app.cfg.agent.context.session_idle_timeout_secs)
    }
}
