mod attention;
mod conversation;
mod dispatch;
mod event_loop;
mod proxy;
mod scheduler;

use std::sync::Arc;

pub use proxy::SessionHandle;
use tokio::{
    sync::Mutex,
    time::{Duration, Instant},
};

use self::scheduler::EventScheduler;
pub(crate) use self::{
    conversation::{Conversation, RunInput},
    proxy::HeartbeatTask,
};
use super::{AgentEngine, run::AgentRuntime, shell::ShellRuntime};
use crate::{
    agent::link::PlatformHandler,
    domain::{model::ChatType, vo::ChatId},
    error::{ErrorKind, Result},
};

// ── Session State ─────────────────────────────────────────────────────────

enum SessionState {
    Idle,
    Busy {
        handle: tokio::task::JoinHandle<()>,
        result_rx: tokio::sync::oneshot::Receiver<crate::agent::runtime::types::BusySignal>,
        started_at: Instant,
    },
}

impl SessionState {
    fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Idle)
    }
}

// ── AgentSession ──────────────────────────────────────────────────────────

pub(super) struct AgentSession {
    runtime: AgentRuntime,
    schedule: EventScheduler,
    conversation: Conversation,
    state: SessionState,
    engine: AgentEngine,
    chat_id: ChatId,
    chat_type: ChatType,
    run_count: usize,
}

impl AgentSession {
    pub async fn new(
        engine: AgentEngine,
        chat_id: ChatId,
        handler: Arc<dyn PlatformHandler>,
        conversation: Conversation,
    ) -> Result<Self> {
        let shell = Arc::new(Mutex::new(ShellRuntime::new(&engine.app.cfg.sandbox)));
        let runtime = AgentRuntime::new(&engine, handler, shell);

        let chat = engine
            .app
            .db
            .srv
            .platform
            .get_chat_by_id(chat_id)
            .await?
            .ok_or_else(|| ErrorKind::Internal.msg(format!("Chat {chat_id} not found")))?;
        let chat_type = chat
            .chat_type()
            .ok_or_else(|| ErrorKind::Internal.msg(format!("Chat {chat_id} has no type")))?;

        Ok(Self {
            runtime,
            schedule: EventScheduler::new(&engine.app.cfg.agent.attention),
            conversation,
            state: SessionState::Idle,
            engine,
            chat_id,
            chat_type,
            run_count: 0,
        })
    }

    pub fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.engine.app.cfg.agent.context.session_idle_timeout_secs)
    }
}
