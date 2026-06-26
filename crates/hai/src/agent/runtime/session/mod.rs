mod assembly;
mod proxy;

use std::sync::Arc;

pub use proxy::{ChatSessionHandle, spawn_chat_session};
use tokio::{
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
    time::{Duration, Instant},
};

use super::{
    AgentEngine,
    event::{
        WakeEvent,
        scheduler::{EventScheduler, PollOutcome},
    },
    round::{Round, RoundTaskPayload},
    shell::ShellRuntime,
};
use crate::{
    agent::{link::BotHandle, runtime::ctx::RoundContext},
    config::schema::SessionConfig,
    domain::{
        entity::ChatType,
        vo::{AttachmentParser, ChatId},
    },
};

// ─── Running round ──────────────────────────────────────────────────────────

struct RunningRound {
    handle: JoinHandle<()>,
    result_rx: oneshot::Receiver<proxy::RoundSignal>,
    started_at: Instant,
}

// ─── Running outcome ────────────────────────────────────────────────────────

enum RunningOutcome {
    Wake(WakeEvent),
    Status(oneshot::Sender<proxy::SessionStatus>),
    Success(Round),
    Failed,
    Cancelled,
}

// ─── Session Loop ────────────────────────────────────────────────────────────

struct SessionLoop {
    schedule: EventScheduler,
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
    async fn new(
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

    async fn run(
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

    // ─── Phase Handlers ───────────────────────────────────────────────────

    async fn step_idle(
        &mut self,
        wake_rx: &mut mpsc::UnboundedReceiver<WakeEvent>,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) -> bool {
        let deadline = self.schedule.next_deadline(self.idle_timeout());

        match deadline {
            Some(t) => {
                tokio::select! {
                    Some(wake) = wake_rx.recv() => {
                let is_rapid = wake.reason.is_rapid();
                self.schedule.push(wake);
                if is_rapid
                    && let PollOutcome::Dispatch(events) = self.schedule.poll(self.idle_timeout())
                {
                    self.dispatch_with(events).await;
                }
                    }
                    Some(query) = status_rx.recv() => {
                        self.answer_status(query, false, None);
                    }
                    _ = tokio::time::sleep_until(t) => match self.schedule.poll(self.idle_timeout()) {
                        PollOutcome::Dispatch(events) => self.dispatch_with(events).await,
                        PollOutcome::Expired => {
                            tracing::info!(chat_id = %self.chat_id, "Session expired, shutting down");
                            return false;
                        }
                        PollOutcome::Wait => {},
                    },
                    else => return false,
                }
            }
            None => {
                tokio::select! {
                    Some(wake) = wake_rx.recv() => {
                let is_rapid = wake.reason.is_rapid();
                self.schedule.push(wake);
                if is_rapid
                    && let PollOutcome::Dispatch(events) = self.schedule.poll(self.idle_timeout())
                {
                    self.dispatch_with(events).await;
                }
                    }
                    Some(query) = status_rx.recv() => {
                        self.answer_status(query, false, None);
                    }
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
    ) -> bool {
        let mut round = match self.running.take() {
            Some(r) => r,
            None => return true,
        };

        let outcome = {
            let result_rx = &mut round.result_rx;

            tokio::select! {
                biased;
                Some(wake) = wake_rx.recv() => RunningOutcome::Wake(wake),
                Some(query) = status_rx.recv() => RunningOutcome::Status(query),
                result = &mut *result_rx => match result {
                    Ok(Some(output)) => RunningOutcome::Success(output),
                    Ok(None) => RunningOutcome::Failed,
                    Err(_) => RunningOutcome::Cancelled,
                },
            }
        };

        match outcome {
            RunningOutcome::Wake(wake) => {
                let is_rapid = wake.reason.is_rapid();
                let is_addressed = wake.reason.is_addressed();
                self.schedule.push(wake);
                if is_rapid {
                    round.handle.abort();
                    self.try_dispatch_next().await;
                } else if is_addressed && round.started_at.elapsed() < Duration::from_secs(3) {
                    round.handle.abort();
                } else {
                    self.running = Some(round);
                }
            }
            RunningOutcome::Status(query) => {
                let status = self.build_status(true, Some(round.started_at));
                let _ = query.send(status);
                self.running = Some(round);
            }
            RunningOutcome::Success(output) => {
                self.on_round_complete(output).await;
                self.try_dispatch_next().await;
            }
            RunningOutcome::Failed => {
                tracing::warn!(%self.chat_id, "Round task failed");
            }
            RunningOutcome::Cancelled => {
                tracing::warn!(%self.chat_id, "Round task panicked");
            }
        }
        true
    }

    // ─── Transition helpers ───────────────────────────────────────────────

    async fn on_round_complete(&mut self, output: Round) {
        let did_send = output
            .tool_calls
            .iter()
            .any(|t| matches!(t.tool_name.as_str(), "send_message" | "send_voice"));
        self.rounds.push(output);
        if did_send {
            self.schedule.refresh();
        }
        if matches!(
            self.engine.app.cfg.agent.context.session,
            SessionConfig::SingleRound
        ) {
            self.rounds.clear();
        }
    }

    fn build_status(
        &mut self,
        round_running: bool,
        round_started: Option<Instant>,
    ) -> proxy::SessionStatus {
        proxy::SessionStatus {
            scheduler: self.schedule.snapshot(),
            rounds_completed: self.rounds.len(),
            round_running,
            round_elapsed_secs: round_started.map(|t| t.elapsed().as_secs_f64()),
            model: self.engine.app.cfg.agent.model.clone(),
        }
    }

    fn answer_status(
        &mut self,
        query: oneshot::Sender<proxy::SessionStatus>,
        round_running: bool,
        round_started: Option<Instant>,
    ) {
        let status = self.build_status(round_running, round_started);
        let _ = query.send(status);
    }

    // ─── Dispatch ─────────────────────────────────────────────────────────

    /// 从 scheduler 获取下一批 events 并派发。用于 round 完成后或中断后链式调度。
    async fn try_dispatch_next(&mut self) {
        if self.running.is_some() {
            return;
        }
        if let PollOutcome::Dispatch(events) = self.schedule.poll(self.idle_timeout()) {
            self.dispatch_with(events).await;
        }
    }

    async fn dispatch_with(&mut self, events: Vec<WakeEvent>) {
        if self.running.is_some() {
            return;
        }

        let Some((ctx, payload)) = self.assemble_round(events).await else {
            return;
        };

        let (handle, rx) = Self::spawn_round_task(self.engine.clone(), ctx, payload);

        self.running = Some(RunningRound {
            handle,
            result_rx: rx,
            started_at: Instant::now(),
        });
    }

    fn spawn_round_task(
        engine: AgentEngine,
        ctx: RoundContext,
        payload: RoundTaskPayload,
    ) -> (JoinHandle<()>, oneshot::Receiver<proxy::RoundSignal>) {
        let bot = ctx.bot.clone();
        let chat_id = ctx.chat_id;
        let events_reasons: Vec<&str> = ctx.events.iter().map(|e| e.reason.label()).collect();
        let prompt = if tracing::enabled!(tracing::Level::INFO) {
            Some(payload.prompt.clone())
        } else {
            None
        };

        let (tx, rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            tracing::info!(%chat_id, reasons = ?events_reasons, "Agent woke up");
            if let Some(p) = &prompt {
                tracing::info!(%chat_id, "Agent task message:\n{p}");
            }

            let _hb = proxy::HeartbeatTask::spawn(bot, chat_id);
            let result = engine.run(&ctx, payload.prompt).await;

            match result {
                Ok(output) => {
                    if !payload.message_ids.is_empty()
                        && let Err(e) = ctx
                            .app
                            .db
                            .srv
                            .message
                            .mark_unread_seen(&payload.message_ids)
                            .await
                    {
                        tracing::error!(%chat_id, "Failed to mark messages seen: {e}");
                    }
                    let _ = tx.send(Some(Round {
                        segment: payload.segment,
                        tool_calls: output.tool_calls,
                        since_id: payload.since_id,
                    }));
                }
                Err(e) => {
                    tracing::error!(%chat_id, "Agent run failed: {e}");
                    let _ = tx.send(None);
                }
            }
        });

        (handle, rx)
    }
}
