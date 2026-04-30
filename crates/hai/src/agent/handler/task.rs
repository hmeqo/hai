use std::sync::Arc;

use tokio::task::{JoinError, JoinSet};

use super::{AgentCtx, debounce::Debouncer};
use crate::agent::{event::AgentEvents, round::RoundContext};

pub struct ActiveTask {
    tasks: JoinSet<()>,
    interruptible: bool,
}

impl ActiveTask {
    pub fn idle() -> Self {
        Self {
            tasks: JoinSet::new(),
            interruptible: false,
        }
    }

    pub fn is_running(&self) -> bool {
        !self.tasks.is_empty()
    }

    pub fn spawn(&mut self, ctx: Arc<AgentCtx>, rc: RoundContext) {
        self.interruptible = rc.events.all_interruptible();
        self.tasks.spawn(async move {
            let chat_id = rc.chat_id;
            if let Err(e) = ctx.execute(rc).await {
                tracing::error!(chat_id, "Agent task failed: {e}");
            }
        });
    }

    pub fn try_interrupt(&mut self, debouncer: &Debouncer) -> bool {
        if !self.is_running() || !self.interruptible {
            return false;
        }
        if debouncer.is_within_window() {
            tracing::debug!("Interruptible task aborted by incoming event.");
            self.tasks.abort_all();
        }
        true
    }

    pub async fn join_next(&mut self) -> Option<Result<(), JoinError>> {
        self.tasks.join_next().await
    }

    pub fn on_finished(&mut self, chat_id: i64, result: Result<(), JoinError>) {
        self.interruptible = false;
        match result {
            Ok(()) => {}
            Err(e) if e.is_cancelled() => tracing::debug!(chat_id, "Agent task aborted."),
            Err(e) => tracing::error!(chat_id, "Agent task panicked: {e}"),
        }
    }

    pub async fn drain(self) {
        self.tasks.join_all().await;
    }
}
