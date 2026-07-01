//! RoundContext 工厂 —— 从 events 构建执行上下文。

use super::SessionLoop;
use crate::agent::{event::WakeEvent, runtime::ctx::RoundContext};

impl SessionLoop {
    pub(super) fn build_round_context(&self, events: Vec<WakeEvent>) -> RoundContext {
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
            enabled_parsers: self.enabled_parsers.clone(),
            tts_enabled: self.tts_enabled,
        }
    }
}
