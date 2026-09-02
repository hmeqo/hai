use std::{collections::HashMap, sync::Arc};

use tokio::{
    sync::{RwLock, mpsc},
    task::JoinHandle,
};

use super::{
    AgentEngine,
    event::Inbox,
    session::{Conversation, SessionHandle},
};
use crate::{agent::link::PlatformHandler, domain::vo::ChatId, error::Result};

#[derive(Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<ChatId, SessionEntry>>>,
    handler: Arc<dyn PlatformHandler>,
    engine: AgentEngine,
}

struct SessionEntry {
    handle: SessionHandle,
    task: Arc<JoinHandle<()>>,
}

impl SessionManager {
    pub fn new(handler: Arc<dyn PlatformHandler>, engine: AgentEngine) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            handler,
            engine,
        }
    }

    /// 按需创建/复用会话执行环境：dead 条目先清理，存活则复用，否则重建
    /// （恢复 = 重建环境 + 载入对话状态；DB 违约上抛，不吞错）。
    pub async fn get_or_create(&self, chat_id: ChatId) -> Result<SessionHandle> {
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, e| !e.task.is_finished());
        if let Some(entry) = sessions.get(&chat_id) {
            return Ok(entry.handle.clone());
        }
        let entry = self.spawn(chat_id).await?;
        let handle = entry.handle.clone();
        sessions.insert(chat_id, entry);
        Ok(handle)
    }

    async fn spawn(&self, chat_id: ChatId) -> Result<SessionEntry> {
        let record = self.engine.app.db.srv.conversation.get(chat_id).await?;
        let conversation = match record {
            Some(r) => {
                // updated_at（最后保存 ≈ 最后活动）作为恢复超期判定基准（章节重开）
                Conversation::from_snapshot(
                    self.engine.app.db.srv.conversation.restore(&r),
                    Some(r.updated_at),
                )
            }
            None => Conversation::new(),
        };

        let inbox = Inbox::new();
        let (status_tx, status_rx) = mpsc::unbounded_channel();

        let engine = self.engine.clone();
        let handler = self.handler.clone();
        let inbox_for_turn = inbox.clone();
        let sessions_arc = self.sessions.clone();

        let task = tokio::spawn(async move {
            let session =
                super::session::AgentSession::new(engine, chat_id, handler, conversation).await;
            match session {
                Ok(mut s) => s.run(inbox_for_turn, status_rx).await,
                Err(e) => tracing::error!(%chat_id, "Failed to create session: {e}"),
            }
            // 即时清理：环境任务结束即从 registry 移除（对话状态留在 DB，永存）
            sessions_arc.write().await.remove(&chat_id);
        });

        let join = Arc::new(task);

        let handle = SessionHandle {
            chat_id,
            inbox,
            status_tx,
            join: join.clone(),
        };

        Ok(SessionEntry { handle, task: join })
    }
}
