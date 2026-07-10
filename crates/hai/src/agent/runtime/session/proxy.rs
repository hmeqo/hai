use std::sync::Arc;

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use self::super::scheduler::SchedulerStatus;
use crate::{
    agent::{
        event::WakeEvent,
        runtime::{event::Inbox, types::RunOutput},
    },
    domain::vo::ChatId,
};

pub(super) type RunSignal = Option<RunOutput>;

pub struct SessionStatus {
    pub scheduler: SchedulerStatus,
    pub turns_count: usize,
    pub prompt_tokens: u32,
    pub conversation_msgs: usize,
    pub mode: &'static str,
    pub run_in_progress: bool,
    pub run_elapsed_secs: Option<f64>,
    pub model: String,
    pub last_turns: Option<Vec<super::super::types::Turn>>,
}

#[derive(Clone)]
pub struct SessionHandle {
    pub chat_id: ChatId,
    pub inbox: Inbox,
    pub status_tx: mpsc::UnboundedSender<oneshot::Sender<SessionStatus>>,
    pub(crate) join: Arc<JoinHandle<()>>,
}

impl SessionHandle {
    pub fn wake(&self, event: WakeEvent) {
        self.inbox.push(event);
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
        !self.join.is_finished()
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
