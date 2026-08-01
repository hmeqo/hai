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
use crate::{agent::link::PlatformHandler, domain::vo::ChatId};

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
        let conversation = self
            .engine
            .app
            .db
            .srv
            .conversation
            .get(chat_id)
            .await
            .ok()
            .flatten()
            .map(|r| Conversation::from_snapshot(self.engine.app.db.srv.conversation.restore(&r)))
            .unwrap_or_else(Conversation::new);

        let inbox = Inbox::new();
        let (status_tx, status_rx) = mpsc::unbounded_channel();

        let engine = self.engine.clone();
        let handler = self.handler.clone();
        let inbox_for_run = inbox.clone();

        let task = tokio::spawn(async move {
            let session =
                super::session::AgentSession::new(engine, chat_id, handler, conversation).await;
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
