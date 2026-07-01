use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use self::super::scheduler::SchedulerStatus;
use super::super::types::{RunOutput, Turn};
use crate::{agent::event::WakeEvent, domain::vo::ChatId};

pub(super) type RunSignal = Option<RunOutput>;

pub struct SessionStatus {
    pub scheduler: SchedulerStatus,
    pub runs_completed: usize,
    pub run_in_progress: bool,
    pub run_elapsed_secs: Option<f64>,
    pub model: String,
    pub last_run_turns: Option<Vec<Turn>>,
}

#[derive(Clone)]
pub struct SessionHandle {
    pub chat_id: ChatId,
    pub wake_tx: mpsc::UnboundedSender<WakeEvent>,
    pub status_tx: mpsc::UnboundedSender<oneshot::Sender<SessionStatus>>,
}

impl SessionHandle {
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

pub(super) struct HeartbeatTask(JoinHandle<()>);

impl HeartbeatTask {
    pub fn spawn(bot: crate::agent::link::BotHandle, chat_id: ChatId) -> Self {
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
