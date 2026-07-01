mod context;
mod dispatch;
mod messages;
mod proxy;
mod round;

use std::sync::Arc;

use genai::chat::ChatMessage;
pub use proxy::ChatSessionHandle;
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
    time::{Duration, Instant},
};

use super::{
    AgentEngine,
    event::{WakeEvent, scheduler::EventScheduler},
    round::Round,
    shell::ShellRuntime,
};
use crate::{
    agent::link::BotHandle,
    domain::{
        model::ChatType,
        vo::{AttachmentParser, ChatId},
    },
};

// ── 运行中的轮次 ─────────────────────────────────────────────────────────────

struct RunningRound {
    handle: JoinHandle<()>,
    result_rx: oneshot::Receiver<proxy::RoundSignal>,
    started_at: Instant,
}

impl RunningRound {
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

// ── select! 轮询结果 ─────────────────────────────────────────────────────────

enum RunningOutcome {
    Wake(WakeEvent),
    Status(oneshot::Sender<proxy::SessionStatus>),
    Success(Round),
    Failed,
    Cancelled,
}

// ── Session 状态机 ───────────────────────────────────────────────────────────

pub(super) struct SessionLoop {
    schedule: EventScheduler,
    messages: Vec<ChatMessage>,
    rounds: Vec<Round>,
    running: Option<RunningRound>,
    engine: AgentEngine,
    enabled_parsers: Vec<&'static str>,
    tts_enabled: bool,
    chat_id: ChatId,
    chat_type: ChatType,
    bot: BotHandle,
    shell: Arc<Mutex<ShellRuntime>>,
}

impl SessionLoop {
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

        Self {
            schedule: EventScheduler::new(base_heat, window_secs),
            messages: Vec::new(),
            rounds: Vec::new(),
            running: None,
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

// ── 事件循环 ─────────────────────────────────────────────────────────────────

impl SessionLoop {
    pub(super) async fn run(
        &mut self,
        mut wake_rx: mpsc::UnboundedReceiver<WakeEvent>,
        mut status_rx: mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) {
        loop {
            if self.running.is_some() {
                self.step_running(&mut wake_rx, &mut status_rx).await;
            } else if !self.step_idle(&mut wake_rx, &mut status_rx).await {
                break;
            }
        }
    }

    async fn step_idle(
        &mut self,
        wake_rx: &mut mpsc::UnboundedReceiver<WakeEvent>,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) -> bool {
        let timeout = self.idle_timeout();
        match self.schedule.next_deadline(timeout) {
            Some(t) => {
                tokio::select! {
                    Some(wake) = wake_rx.recv() => self.on_wake(wake).await,
                    Some(query) = status_rx.recv() => self.answer_status(query, false, None),
                    _ = tokio::time::sleep_until(t) => return self.handle_deadline().await,
                    else => return false,
                }
            }
            None => {
                tokio::select! {
                    Some(wake) = wake_rx.recv() => self.on_wake(wake).await,
                    Some(query) = status_rx.recv() => self.answer_status(query, false, None),
                    else => return false,
                }
            }
        }
        true
    }

    async fn step_running(
        &mut self,
        wake_rx: &mut mpsc::UnboundedReceiver<WakeEvent>,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) {
        let mut round = match self.running.take() {
            Some(r) => r,
            None => return,
        };

        let outcome = round.poll(wake_rx, status_rx).await;

        match outcome {
            RunningOutcome::Wake(wake) => {
                let is_rapid = wake.reason.is_rapid();
                let is_addressed = wake.reason.is_addressed();
                self.schedule.push(wake);
                if is_rapid {
                    round.abort();
                    self.try_dispatch_next().await;
                } else if is_addressed && round.started_at.elapsed() < Duration::from_secs(3) {
                    round.abort();
                } else {
                    self.running = Some(round);
                }
            }
            RunningOutcome::Status(query) => {
                self.answer_status(query, true, Some(round.started_at));
                self.running = Some(round);
            }
            RunningOutcome::Success(output) => {
                self.on_round_complete(output).await;
                self.try_dispatch_next().await;
            }
            RunningOutcome::Failed => {
                let elapsed = round.started_at.elapsed();
                tracing::warn!(
                    %self.chat_id,
                    elapsed_secs = %elapsed.as_secs_f64(),
                    "Round failed",
                );
                self.try_dispatch_next().await;
            }
            RunningOutcome::Cancelled => {
                let elapsed = round.started_at.elapsed();
                tracing::warn!(
                    %self.chat_id,
                    elapsed_secs = %elapsed.as_secs_f64(),
                    "Round panicked",
                );
                self.try_dispatch_next().await;
            }
        }
    }
}
