use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

use self::super::scheduler::SchedulerStatus;
use crate::{
    agent::{event::WakeEvent, link::PlatformHandler, runtime::event::Inbox},
    domain::vo::ChatId,
};

pub struct SessionStatus {
    pub scheduler: SchedulerStatus,
    pub turns_count: usize,
    pub context_tokens: u32,
    pub conversation_msgs: usize,
    pub run_in_progress: bool,
    pub run_elapsed_secs: Option<f64>,
    pub model: String,
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

pub(crate) struct HeartbeatTask(JoinHandle<()>);

impl HeartbeatTask {
    pub fn spawn(handler: Arc<dyn PlatformHandler>, chat_id: ChatId) -> Self {
        Self(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                handler.send_typing(chat_id).await;
            }
        }))
    }
}

impl Drop for HeartbeatTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
