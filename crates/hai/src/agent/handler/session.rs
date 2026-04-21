use std::sync::Arc;

use tokio::sync::mpsc;
use tokio::time::{Duration, Instant, sleep_until};

use crate::agent::event::AgentEvent;

use super::{AgentHandler, debounce::Debouncer, task::ActiveTask};

/// 用于"挂起" sleep 的远期时间点，避免 Duration::MAX 溢出
const FAR_FUTURE: Duration = Duration::from_secs(365 * 24 * 3600 * 30);

/// 为指定 chat 启动独立的 session actor，返回其事件入口
pub fn spawn_chat_session(
    handler: Arc<AgentHandler>,
    chat_id: i64,
) -> mpsc::UnboundedSender<AgentEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    let debounce_min = Duration::from_millis(handler.config.agent.debounce_ms);
    tokio::spawn(ChatSession::new(handler, chat_id, rx, debounce_min).run());
    tx
}

/// 单 chat 状态机
///
/// 三条并发分支：
///   1. 事件接收（始终开启）
///   2. 当前任务完成
///   3. 防抖窗口到期 → spawn 任务
///
/// 窗口期语义（基准时刻 = 第一个事件入队时，持续 DEBOUNCE_MAX）：
///   · 普通事件在窗口期内：每次新事件将 deadline 滚动至 now + min_wait，
///     但不超过窗口期截止时间；若任务已 spawn 则打断并重新防抖
///   · 普通事件在窗口期外：不打断运行中的任务，事件积压等任务完成
///   · bypass（rapid）事件：跳过防抖等待（deadline = now），但仍受窗口期限制——
///     窗口期内可打断并立即触发，窗口期外不打断运行中的任务，等完成后立即触发
struct ChatSession {
    handler: Arc<AgentHandler>,
    chat_id: i64,
    rx: mpsc::UnboundedReceiver<AgentEvent>,
    debouncer: Debouncer,
    active: ActiveTask,
}

impl ChatSession {
    fn new(
        handler: Arc<AgentHandler>,
        chat_id: i64,
        rx: mpsc::UnboundedReceiver<AgentEvent>,
        debounce_min: Duration,
    ) -> Self {
        Self {
            handler,
            chat_id,
            rx,
            debouncer: Debouncer::new(debounce_min),
            active: ActiveTask::idle(),
        }
    }

    async fn run(mut self) {
        let chat_id = self.chat_id;

        // 固定分配的 sleep future；初始挂在远期，由 guard 阻止防抖分支触发
        let mut debounce_sleep = Box::pin(sleep_until(Instant::now() + FAR_FUTURE));

        loop {
            tokio::select! {
                // 分支 1：持续收集新事件，不受任务或防抖状态影响
                msg = self.rx.recv() => match msg {
                    None => {
                        tracing::info!(chat_id, "Session channel closed.");
                        self.active.drain().await;
                        break;
                    }
                    Some(event) => {
                        let interrupted = self.active.try_interrupt(&self.debouncer);
                        self.debouncer.push(event);
                        if interrupted || !self.active.is_running() {
                            // 任务被打断，或本来就无任务：更新防抖窗口
                            debounce_sleep.as_mut().reset(self.debouncer.next_deadline());
                        }
                        // 不可打断任务正在运行：事件入队，等任务完成后恢复防抖
                    }
                },

                // 分支 2：当前任务结束（成功 / 失败 / 被 abort）
                Some(result) = self.active.join_next() => {
                    self.active.on_finished(chat_id, result);
                    if !self.debouncer.is_empty() {
                        debounce_sleep.as_mut().reset(self.debouncer.next_deadline());
                    }
                },

                // 分支 3：防抖窗口到期 → flush 并启动新任务
                // guard：仅在有积压且无运行任务时触发
                _ = &mut debounce_sleep,
                    if !self.debouncer.is_empty() && !self.active.is_running() =>
                {
                    let events = self.debouncer.flush();
                    tracing::debug!(chat_id, n = events.len(), "Debounce expired, spawning task.");
                    self.active.spawn(Arc::clone(&self.handler), chat_id, events);
                },
            }
        }
    }
}
