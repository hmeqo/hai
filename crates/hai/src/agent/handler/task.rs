use std::sync::Arc;

use tokio::task::{JoinError, JoinSet};

use super::{AgentHandler, debounce::Debouncer};
use crate::agent::event::{AgentEvent, AgentEvents};

/// 当前运行任务的句柄
///
/// 用 JoinSet（容量 ≤ 1）持有 tokio 任务，避免对 Option<JoinHandle> 的
/// 手动借用管理，也消除了 OptionFuture 带来的跨分支 &mut 借用冲突。
pub struct ActiveTask {
    tasks: JoinSet<()>,
    /// 当前任务的事件类型是否允许被中断
    interruptible_by_type: bool,
}

impl ActiveTask {
    pub fn idle() -> Self {
        Self {
            tasks: JoinSet::new(),
            interruptible_by_type: false,
        }
    }

    pub fn is_running(&self) -> bool {
        !self.tasks.is_empty()
    }

    pub fn spawn(&mut self, handler: Arc<AgentHandler>, chat_id: i64, events: Vec<AgentEvent>) {
        self.interruptible_by_type = events.all_interruptible();
        self.tasks.spawn(async move {
            if let Err(e) = handler.execute(chat_id, &events).await {
                tracing::error!(chat_id, "Agent task failed: {e}");
            }
        });
    }

    pub fn try_interrupt(&mut self, debouncer: &Debouncer) -> bool {
        if !self.is_running() || !self.interruptible_by_type {
            return false;
        }
        // 仅在防抖窗口期内打断
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
        self.interruptible_by_type = false;
        match result {
            Ok(()) => {}
            Err(e) if e.is_cancelled() => tracing::debug!(chat_id, "Agent task aborted."),
            Err(e) => tracing::error!(chat_id, "Agent task panicked: {e}"),
        }
    }

    /// Graceful shutdown：等待任务自然结束
    pub async fn drain(self) {
        self.tasks.join_all().await;
    }
}
