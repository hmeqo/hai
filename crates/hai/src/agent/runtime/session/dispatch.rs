//! 事件调度：接收 wake 事件、deadline 到期 → 派发 agent task。
//!
//! 两种入口：
//! - `try_dispatch` —— run 完成或打断后尝试派发下一批
//! - `dispatch_with` —— 实际组装 + 启动 agent task

use super::{
    super::event::WakeEvent, ActiveRun, AgentSession, SessionState, scheduler::PollOutcome,
};

impl AgentSession {
    pub(super) async fn try_dispatch(&mut self) {
        if let PollOutcome::Dispatch(events) = self.schedule.poll(self.idle_timeout()) {
            self.dispatch_with(events).await;
        } else {
            self.state = SessionState::Idle;
        }
    }

    pub(super) async fn dispatch_with(&mut self, events: Vec<WakeEvent>) {
        let Some((ctx, payload)) = self.assemble_run(events).await else {
            self.state = SessionState::Idle;
            return;
        };

        let (handle, result_rx) = super::run::spawn_run_task(self.engine.clone(), ctx, payload);

        self.state = SessionState::Active(ActiveRun {
            handle,
            result_rx,
            started_at: tokio::time::Instant::now(),
        });
    }
}
