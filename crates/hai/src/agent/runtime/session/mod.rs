mod attention;
mod conversation;
mod dispatch;
mod event_loop;
mod prompt;
mod proxy;
mod scheduler;

use std::sync::Arc;

pub use proxy::SessionHandle;
use tokio::{sync::Mutex, time::Duration};

use self::{conversation::Conversation, event_loop::ActiveRun, scheduler::EventScheduler};
use super::{AgentEngine, shell::ShellRuntime};
use crate::{
    agent::link::PlatformHandler,
    domain::{
        model::ChatType,
        vo::{AttachmentParser, ChatId},
    },
    error::{ErrorKind, Result},
};

// ── Session State ─────────────────────────────────────────────────────────

enum SessionState {
    Idle,
    Active(ActiveRun),
}

impl SessionState {
    fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Idle)
    }
}

// ── AgentSession ──────────────────────────────────────────────────────────

pub(super) struct AgentSession {
    schedule: EventScheduler,
    conversation: Conversation,
    state: SessionState,
    engine: AgentEngine,
    enabled_parsers: Vec<&'static str>,
    tts_enabled: bool,
    chat_id: ChatId,
    chat_type: ChatType,
    handler: Arc<dyn PlatformHandler>,
    shell: Arc<Mutex<ShellRuntime>>,
}

impl AgentSession {
    pub(super) async fn new(
        engine: AgentEngine,
        chat_id: ChatId,
        handler: Arc<dyn PlatformHandler>,
        shell: Arc<Mutex<ShellRuntime>>,
        base_heat: f64,
        window_secs: f64,
    ) -> Result<Self> {
        let mc = &engine.app.cfg.multimodal;
        let mut enabled_parsers = Vec::new();
        if mc.input.audio.enabled() {
            enabled_parsers.push(AttachmentParser::Audio.name());
        }
        if mc.input.video.enabled() {
            enabled_parsers.push(AttachmentParser::Video.name());
        }
        if mc.input.image.enabled() {
            enabled_parsers.push(AttachmentParser::Image.name());
        }
        let tts_enabled = mc.tts.enabled();

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

        let conversation =
            Conversation::new(engine.app.cfg.agent.context.conversation_mode.clone());

        Ok(Self {
            schedule: EventScheduler::new(base_heat, window_secs),
            conversation,
            state: SessionState::Idle,
            engine,
            chat_id,
            chat_type,
            handler,
            shell,
            enabled_parsers,
            tts_enabled,
        })
    }

    pub(super) fn idle_timeout(&self) -> Duration {
        Duration::from_secs(self.engine.app.cfg.agent.context.session_idle_timeout_secs)
    }
}
