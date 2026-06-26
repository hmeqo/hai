use std::sync::Arc;

use tokio::{
    sync::{Mutex, mpsc, oneshot},
    task::JoinHandle,
};

use super::super::{AgentEngine, event::scheduler::SchedulerStatus, shell::ShellRuntime};
use crate::{
    agent::{event::WakeEvent, link::BotHandle},
    domain::vo::ChatId,
};

type WakeSender = mpsc::UnboundedSender<WakeEvent>;
pub(super) type RoundSignal = Option<super::super::round::Round>;
type StatusSender = mpsc::UnboundedSender<oneshot::Sender<SessionStatus>>;

pub struct SessionStatus {
    pub scheduler: SchedulerStatus,
    pub rounds_completed: usize,
    pub round_running: bool,
    pub round_elapsed_secs: Option<f64>,
    pub model: String,
}

#[derive(Clone)]
pub struct ChatSessionHandle {
    pub chat_id: ChatId,
    wake_tx: WakeSender,
    status_tx: StatusSender,
}

impl ChatSessionHandle {
    pub fn wake(&self, event: WakeEvent) {
        if let Err(e) = self.wake_tx.send(event) {
            tracing::error!(chat_id = %self.chat_id, "Failed to send wake event: {e}");
        }
    }

    pub async fn status(&self) -> Option<SessionStatus> {
        let (tx, rx) = oneshot::channel();
        if self.status_tx.send(tx).is_err() {
            tracing::warn!(chat_id = %self.chat_id, "Failed to send status query: session dead");
            return None;
        }
        match rx.await {
            Ok(s) => Some(s),
            Err(_) => {
                tracing::warn!(chat_id = %self.chat_id, "Status query cancelled: session dropped");
                None
            }
        }
    }

    pub fn is_alive(&self) -> bool {
        !self.wake_tx.is_closed()
    }
}

pub fn spawn_chat_session(
    chat_id: ChatId,
    bot: BotHandle,
    engine: AgentEngine,
    base_heat: f64,
    window_secs: f64,
) -> ChatSessionHandle {
    let (wake_tx, wake_rx) = mpsc::unbounded_channel();
    let (status_tx, status_rx) = mpsc::unbounded_channel();
    let shell = Arc::new(Mutex::new(ShellRuntime::new(&engine.app.cfg.sandbox)));

    let handle = ChatSessionHandle {
        chat_id,
        wake_tx,
        status_tx,
    };

    let session = tokio::spawn(async move {
        super::SessionLoop::new(engine, chat_id, bot, shell, base_heat, window_secs)
            .await
            .run(wake_rx, status_rx)
            .await;
    });

    tokio::spawn(async move {
        if let Err(e) = session.await {
            tracing::error!(chat_id = %chat_id, "Session loop panicked: {e}");
        }
    });

    handle
}

pub(super) struct HeartbeatTask(JoinHandle<()>);

impl HeartbeatTask {
    pub fn spawn(bot: BotHandle, chat_id: ChatId) -> Self {
        Self(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                bot.send_typing(chat_id).await;
            }
        }))
    }
}

impl Drop for HeartbeatTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
