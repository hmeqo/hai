use std::{collections::HashMap, sync::Arc};

use tokio::{
    sync::{Mutex, RwLock, mpsc},
    task::JoinHandle,
};

use super::{AgentEngine, session::SessionHandle, shell::ShellRuntime};
use crate::{agent::link::BotHandle, domain::vo::ChatId};

pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<ChatId, SessionEntry>>>,
    cleanup_tx: mpsc::UnboundedSender<ChatId>,
    bot: BotHandle,
    engine: AgentEngine,
}

struct SessionEntry {
    handle: SessionHandle,
    task: JoinHandle<()>,
}

impl SessionManager {
    pub fn new(bot: BotHandle, engine: AgentEngine) -> Self {
        let (cleanup_tx, mut cleanup_rx) = mpsc::unbounded_channel::<ChatId>();
        let sessions: Arc<RwLock<HashMap<ChatId, SessionEntry>>> =
            Arc::new(RwLock::new(HashMap::new()));

        let bg_sessions = Arc::clone(&sessions);
        tokio::spawn(async move {
            while let Some(chat_id) = cleanup_rx.recv().await {
                let mut write = bg_sessions.write().await;
                if let Some(entry) = write.get(&chat_id) {
                    if entry.task.is_finished() {
                        write.remove(&chat_id);
                        tracing::debug!(%chat_id, "Session auto-cleaned");
                    }
                }
            }
        });

        Self {
            sessions,
            cleanup_tx,
            bot,
            engine,
        }
    }

    pub async fn get_or_create(&self, chat_id: ChatId) -> SessionHandle {
        if let Some(entry) = self.sessions.read().await.get(&chat_id) {
            if entry.handle.is_alive() {
                return entry.handle.clone();
            }
        }

        let entry = self.spawn(chat_id).await;
        let handle = entry.handle.clone();
        let mut write = self.sessions.write().await;
        write.insert(chat_id, entry);
        handle
    }

    async fn spawn(&self, chat_id: ChatId) -> SessionEntry {
        let attention_cfg = &self.engine.app.cfg.agent.attention;
        let personality = &self.engine.app.cfg.agent.personality;
        let base_heat = personality.base_attention(attention_cfg);
        let window_secs = personality.attention_window_secs();

        let (wake_tx, wake_rx) = mpsc::unbounded_channel();
        let (status_tx, status_rx) = mpsc::unbounded_channel();
        let shell =
            std::sync::Arc::new(Mutex::new(ShellRuntime::new(&self.engine.app.cfg.sandbox)));

        let handle = SessionHandle {
            chat_id,
            wake_tx,
            status_tx,
        };

        tracing::info!(%chat_id, "Chat session started");

        let cleanup_tx = self.cleanup_tx.clone();
        let task = tokio::spawn({
            let engine = self.engine.clone();
            let bot = self.bot.clone();
            async move {
                super::session::AgentSession::new(
                    engine,
                    chat_id,
                    bot,
                    shell,
                    base_heat,
                    window_secs,
                )
                .await
                .run(wake_rx, status_rx)
                .await;

                let _ = cleanup_tx.send(chat_id);
            }
        });

        SessionEntry { handle, task }
    }
}
