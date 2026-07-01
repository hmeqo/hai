mod attention;
mod context;
mod conversation;
mod dispatch;
mod messages;
mod proxy;
mod run;
mod scheduler;

use std::sync::Arc;

pub use proxy::SessionHandle;
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
    time::{Duration, Instant},
};

use self::{
    conversation::Conversation,
    scheduler::{EventScheduler, PollOutcome},
};
use super::{AgentEngine, event::WakeEvent, shell::ShellRuntime, types::RunOutput};
use crate::{
    agent::link::BotHandle,
    config::schema::ConversationMode,
    domain::{
        model::ChatType,
        vo::{AttachmentParser, ChatId},
    },
};

// ── Active Run ─────────────────────────────────────────────────────────────

struct ActiveRun {
    handle: JoinHandle<()>,
    result_rx: oneshot::Receiver<proxy::RunSignal>,
    started_at: Instant,
}

impl ActiveRun {
    async fn poll(
        &mut self,
        wake_rx: &mut mpsc::UnboundedReceiver<WakeEvent>,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) -> RunningOutcome {
        tokio::select! {
            biased;
            Some(wake) = wake_rx.recv() => RunningOutcome::Wake(wake),
            Some(query) = status_rx.recv() => RunningOutcome::Status(query),
            result = &mut self.result_rx => match result {
                Ok(Some(output)) => RunningOutcome::Success(output),
                Ok(None) => RunningOutcome::Failed,
                Err(_) => RunningOutcome::Cancelled,
            },
        }
    }

    fn abort(self) {
        self.handle.abort();
    }
}

// ── Poll Outcome ──────────────────────────────────────────────────────────

enum RunningOutcome {
    Wake(WakeEvent),
    Status(oneshot::Sender<proxy::SessionStatus>),
    Success(RunOutput),
    Failed,
    Cancelled,
}

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

// ── Idle Step ─────────────────────────────────────────────────────────────

enum IdleStep {
    Wake(WakeEvent),
    Status(oneshot::Sender<proxy::SessionStatus>),
    Dispatch(Vec<WakeEvent>),
    Wait,
    Expired,
    Closed,
}

// ── AgentSession ──────────────────────────────────────────────────────────

pub(super) struct AgentSession {
    schedule: EventScheduler,
    conversation: Option<Conversation>,
    state: SessionState,
    engine: AgentEngine,
    enabled_parsers: Vec<&'static str>,
    tts_enabled: bool,
    chat_id: ChatId,
    chat_type: ChatType,
    bot: BotHandle,
    shell: Arc<Mutex<ShellRuntime>>,
}

impl AgentSession {
    pub(super) async fn new(
        engine: AgentEngine,
        chat_id: ChatId,
        bot: BotHandle,
        shell: Arc<Mutex<ShellRuntime>>,
        base_heat: f64,
        window_secs: f64,
    ) -> Self {
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

        let chat_type = match engine.app.db.srv.platform.get_chat_by_id(chat_id).await {
            Ok(Some(c)) => c.chat_type(),
            other => {
                tracing::warn!(%chat_id, ?other, "Chat not found, defaulting to Private");
                ChatType::Private
            }
        };

        let conversation =
            if engine.app.cfg.agent.context.conversation_mode == ConversationMode::Persistent {
                Some(Conversation::new())
            } else {
                None
            };

        Self {
            schedule: EventScheduler::new(base_heat, window_secs),
            conversation,
            state: SessionState::Idle,
            engine,
            chat_id,
            chat_type,
            bot,
            shell,
            enabled_parsers,
            tts_enabled,
        }
    }
}

// ── Event Loop ────────────────────────────────────────────────────────────

impl AgentSession {
    pub(super) async fn run(
        &mut self,
        mut wake_rx: mpsc::UnboundedReceiver<WakeEvent>,
        mut status_rx: mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) {
        loop {
            match self.state.take() {
                SessionState::Idle => match self.poll_idle(&mut wake_rx, &mut status_rx).await {
                    IdleStep::Wake(wake) => {
                        let is_rapid = wake.reason.is_rapid();
                        self.schedule.push(wake);
                        if is_rapid {
                            self.try_dispatch().await;
                        } else {
                            self.state = SessionState::Idle;
                        }
                    }
                    IdleStep::Status(query) => {
                        self.answer_status(query);
                        self.state = SessionState::Idle;
                    }
                    IdleStep::Dispatch(events) => self.dispatch_with(events).await,
                    IdleStep::Wait => self.state = SessionState::Idle,
                    IdleStep::Expired | IdleStep::Closed => break,
                },
                SessionState::Active(mut active) => {
                    let outcome = active.poll(&mut wake_rx, &mut status_rx).await;
                    match outcome {
                        RunningOutcome::Wake(wake) => {
                            let is_rapid = wake.reason.is_rapid();
                            let is_addressed = wake.reason.is_addressed();
                            self.schedule.push(wake);
                            if is_rapid
                                || (is_addressed
                                    && active.started_at.elapsed() < Duration::from_secs(3))
                            {
                                active.abort();
                                self.try_dispatch().await;
                            } else {
                                self.state = SessionState::Active(active);
                            }
                        }
                        RunningOutcome::Status(query) => {
                            self.answer_status(query);
                            self.state = SessionState::Active(active);
                        }
                        RunningOutcome::Success(output) => {
                            let run = output.run;
                            self.on_run_complete(run, output.messages).await;
                            self.try_dispatch().await;
                        }
                        RunningOutcome::Failed => {
                            tracing::warn!(
                                %self.chat_id,
                                elapsed_secs = %active.started_at.elapsed().as_secs_f64(),
                                "Run failed",
                            );
                            self.try_dispatch().await;
                        }
                        RunningOutcome::Cancelled => {
                            tracing::warn!(
                                %self.chat_id,
                                elapsed_secs = %active.started_at.elapsed().as_secs_f64(),
                                "Run panicked",
                            );
                            self.try_dispatch().await;
                        }
                    }
                }
            }
        }
    }

    async fn poll_idle(
        &mut self,
        wake_rx: &mut mpsc::UnboundedReceiver<WakeEvent>,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) -> IdleStep {
        let timeout = self.idle_timeout();
        match self.schedule.next_deadline(timeout) {
            Some(t) => {
                tokio::select! {
                    Some(wake) = wake_rx.recv() => IdleStep::Wake(wake),
                    Some(query) = status_rx.recv() => IdleStep::Status(query),
                    _ = tokio::time::sleep_until(t) => match self.schedule.poll(timeout) {
                        PollOutcome::Dispatch(events) => IdleStep::Dispatch(events),
                        PollOutcome::Expired => {
                            tracing::info!(chat_id = %self.chat_id, "Session expired, shutting down");
                            IdleStep::Expired
                        }
                        PollOutcome::Wait => IdleStep::Wait,
                    },
                    else => IdleStep::Closed,
                }
            }
            None => {
                tokio::select! {
                    Some(wake) = wake_rx.recv() => IdleStep::Wake(wake),
                    Some(query) = status_rx.recv() => IdleStep::Status(query),
                    else => IdleStep::Closed,
                }
            }
        }
    }
}
