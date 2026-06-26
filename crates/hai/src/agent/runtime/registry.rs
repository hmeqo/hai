use std::collections::HashMap;

use tokio::sync::RwLock;

use super::{AgentEngine, ChatSessionHandle};
use crate::{agent::link::BotHandle, domain::vo::ChatId};

/// 管理 ChatSession 的创建与复用
pub struct ChatSessionManager {
    sessions: RwLock<HashMap<ChatId, ChatSessionHandle>>,
    bot: BotHandle,
    engine: AgentEngine,
}

impl ChatSessionManager {
    pub fn new(bot: BotHandle, engine: AgentEngine) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            bot,
            engine,
        }
    }

    pub async fn get_or_create(&self, chat_id: ChatId) -> ChatSessionHandle {
        // 读锁快速路径（微秒级释放）
        if let Some(handle) = self.sessions.read().await.get(&chat_id)
            && handle.is_alive()
        {
            return handle.clone();
        }

        // 无锁创建新 session
        let attention_cfg = &self.engine.app.cfg.agent.attention;
        let personality = &self.engine.app.cfg.agent.personality;
        let base_heat = personality.base_attention(attention_cfg);
        let window_secs = personality.attention_window_secs();
        let handle = super::session::spawn_chat_session(
            chat_id,
            self.bot.clone(),
            self.engine.clone(),
            base_heat,
            window_secs,
        );

        // 短暂写锁：double-check 后插入
        let mut write = self.sessions.write().await;
        if let Some(existing) = write.get(&chat_id) {
            if existing.is_alive() {
                return existing.clone();
            }
            write.remove(&chat_id);
        }
        write.insert(chat_id, handle.clone());
        handle
    }
}
