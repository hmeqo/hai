use std::{collections::HashMap, sync::Arc};

use tokio::{
    sync::{Mutex, RwLock, mpsc},
    task::JoinHandle,
};

use super::{AgentEngine, event::Inbox, session::SessionHandle, shell::ShellRuntime};
use crate::{agent::link::BotHandle, domain::vo::ChatId};

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<ChatId, SessionEntry>>>,
    bot: BotHandle,
    engine: AgentEngine,
}

struct SessionEntry {
    handle: SessionHandle,
    task: Arc<JoinHandle<()>>,
}

impl SessionManager {
    pub fn new(bot: BotHandle, engine: AgentEngine) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            bot,
            engine,
        }
    }

    pub async fn get_or_create(&self, chat_id: ChatId) -> SessionHandle {
        let mut sessions = self.sessions.write().await;
        sessions.retain(|_, e| !e.task.is_finished());
        if let Some(entry) = sessions.get(&chat_id) {
            return entry.handle.clone();
        }
        let entry = self.spawn(chat_id).await;
        let handle = entry.handle.clone();
        sessions.insert(chat_id, entry);
        handle
    }

    async fn spawn(&self, chat_id: ChatId) -> SessionEntry {
        let attention_cfg = &self.engine.app.cfg.agent.attention;
        let personality = &self.engine.app.cfg.agent.personality;
        let base_heat = personality.base_attention(attention_cfg);
        let window_secs = personality.attention_window_secs();

        let inbox = Inbox::new();
        let (status_tx, status_rx) = mpsc::unbounded_channel();
        let shell =
            std::sync::Arc::new(Mutex::new(ShellRuntime::new(&self.engine.app.cfg.sandbox)));

        let engine = self.engine.clone();
        let bot = self.bot.clone();
        let inbox_for_run = inbox.clone();

        let task = tokio::spawn(async move {
            let session = super::session::AgentSession::new(
                engine,
                chat_id,
                bot,
                shell,
                base_heat,
                window_secs,
            )
            .await;
            match session {
                Ok(mut s) => s.run(inbox_for_run, status_rx).await,
                Err(e) => tracing::error!(%chat_id, "Failed to create session: {e}"),
            }
        });

        let join = Arc::new(task);

        let handle = SessionHandle {
            chat_id,
            inbox,
            status_tx,
            join: join.clone(),
        };

        SessionEntry { handle, task: join }
    }
}
