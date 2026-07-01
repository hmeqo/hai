//! 事件调度：接收 wake 事件、deadline 到期 → 派发 agent task。
//!
//! 三种入口：
//! - `on_wake` —— 新事件到达，rapid 事件直发，其余入队等 deadline
//! - `handle_deadline` —— 调度器到期，poll 后派发
//! - `try_dispatch_next` —— round 完成后尝试派发下一批

use super::{
    super::event::{WakeEvent, scheduler::PollOutcome},
    RunningRound,
};
use crate::agent::runtime::session::SessionLoop;

impl SessionLoop {
    /// Wake 事件入队 + rapid 事件直发（跳过防抖+窗口+热度）。
    pub(super) async fn on_wake(&mut self, wake: WakeEvent) {
        let is_rapid = wake.reason.is_rapid();
        self.schedule.push(wake);
        if is_rapid {
            if let PollOutcome::Dispatch(events) = self.schedule.poll(self.idle_timeout()) {
                self.dispatch_with(events).await;
            }
        }
    }

    /// Deadline 到期：触发 schedule poll。
    /// 返回 false 表示 session 过期，应退出循环。
    pub(super) async fn handle_deadline(&mut self) -> bool {
        match self.schedule.poll(self.idle_timeout()) {
            PollOutcome::Dispatch(events) => {
                self.dispatch_with(events).await;
                true
            }
            PollOutcome::Expired => {
                tracing::info!(chat_id = %self.chat_id, "Session expired, shutting down");
                false
            }
            PollOutcome::Wait => true,
        }
    }

    /// 从 scheduler 获取下一批 events 并派发。
    /// 在 round 完成或打断后调用。
    pub(super) async fn try_dispatch_next(&mut self) {
        if self.running.is_some() {
            return;
        }
        if let PollOutcome::Dispatch(events) = self.schedule.poll(self.idle_timeout()) {
            self.dispatch_with(events).await;
        }
    }

    /// 组装并派发一轮 agent task。
    pub(super) async fn dispatch_with(&mut self, events: Vec<WakeEvent>) {
        if self.running.is_some() {
            return;
        }

        let Some((ctx, payload)) = self.assemble_round(events).await else {
            return;
        };

        let (handle, result_rx) = super::round::spawn_round_task(self.engine.clone(), ctx, payload);

        self.running = Some(RunningRound {
            handle,
            result_rx,
            started_at: tokio::time::Instant::now(),
        });
    }
}
