//! Event loop + 状态类型 + build_status

use tokio::sync::{mpsc, oneshot};

use super::{
    super::event::{Inbox, WakeEvents},
    AgentSession, SessionState, proxy,
    scheduler::Decision,
};
use crate::{
    agent::runtime::types::{BusySignal, TurnOutput},
    domain::vo::AgentEventPayload,
};

// ── Busy Outcome ──────────────────────────────────────────────────────────

enum BusyOutcome {
    Status(oneshot::Sender<proxy::SessionStatus>),
    Success(TurnOutput),
    /// 提前正常结束（steering 打断）+ 打断事件（立即续跑）
    Steered(TurnOutput, WakeEvents),
    WrapUpDone(String),
    TurnFailed,
    WrapUpFailed(String),
    Cancelled,
}

// ── Next Step ─────────────────────────────────────────────────────────────

enum NextStep {
    Status(oneshot::Sender<proxy::SessionStatus>),
    Activate(WakeEvents),
    WrapUp,
    Idle,
    Exit,
}

// ── Event Loop ────────────────────────────────────────────────────────────

impl AgentSession {
    pub(crate) async fn run(
        &mut self,
        inbox: Inbox,
        mut status_rx: mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) {
        loop {
            match self.state.take() {
                SessionState::Idle => match self.idle_tick(&inbox, &mut status_rx).await {
                    NextStep::Activate(events) => self.dispatch(events, &inbox).await,
                    NextStep::Status(query) => {
                        self.answer_status(query);
                        self.state = SessionState::Idle;
                    }
                    NextStep::Idle => self.state = SessionState::Idle,
                    NextStep::WrapUp => self.start_wrap_up(&inbox),
                    NextStep::Exit => break,
                },
                SessionState::Busy {
                    handle,
                    mut result_rx,
                    started_at,
                } => {
                    let outcome = tokio::select! {
                        biased;
                        Some(query) = status_rx.recv() => BusyOutcome::Status(query),
                        result = &mut result_rx => match result {
                            Ok(BusySignal::Turn(output)) => BusyOutcome::Success(output),
                            Ok(BusySignal::Steered(output, events)) => {
                                BusyOutcome::Steered(output, events)
                            }
                            Ok(BusySignal::WrapUp(text)) => BusyOutcome::WrapUpDone(text),
                            Ok(BusySignal::TurnFailed) => BusyOutcome::TurnFailed,
                            Ok(BusySignal::WrapUpFailed(err)) => BusyOutcome::WrapUpFailed(err),
                            Err(_) => BusyOutcome::Cancelled,
                        },
                    };
                    match outcome {
                        BusyOutcome::Status(query) => {
                            self.answer_status(query);
                            self.state = SessionState::Busy {
                                handle,
                                result_rx,
                                started_at,
                            };
                        }
                        BusyOutcome::Success(output) => {
                            self.on_complete(output, &inbox).await;
                        }
                        BusyOutcome::Steered(output, steered_events) => {
                            // 提前正常结束：已处理内容全部生效（on_complete 推进 + 落盘）
                            self.on_complete(output, &inbox).await;
                            // 立即增量续跑：打断事件 + inbox 剩余合并派发（不等 idle/debounce）
                            let mut combined = steered_events;
                            combined.extend(inbox.drain());
                            if !combined.is_empty() {
                                self.dispatch(combined, &inbox).await;
                            }
                        }
                        BusyOutcome::WrapUpDone(text) => {
                            self.on_wrap_up_done(text).await;
                            self.drain_into_idle(&inbox);
                        }
                        BusyOutcome::TurnFailed => {
                            // 失败零状态副作用：游标/shown 暂存丢弃（不推进），保持 Idle 等下一条消息
                            self.conversation.discard_since_id();
                            self.conversation.discard_shown();
                            tracing::warn!(%self.chat_id, "Busy task failed");
                            self.drain_into_idle(&inbox);
                        }
                        BusyOutcome::WrapUpFailed(err) => {
                            // 失败也重开干净章节：会话干净不依赖 收尾 成功
                            self.engine
                                .app
                                .event_bus
                                .emit(self.chat_id, AgentEventPayload::WrapUpFailed { error: err });
                            self.conversation.start_new_chapter(None);
                            let snap = self.conversation.snapshot();
                            if let Err(e) = self
                                .engine
                                .app
                                .db
                                .srv
                                .conversation
                                .save(&snap, self.chat_id)
                                .await
                            {
                                tracing::warn!(%self.chat_id, "Failed to save reopened chapter: {e}");
                            }
                            self.drain_into_idle(&inbox);
                        }
                        BusyOutcome::Cancelled => {
                            tracing::warn!(%self.chat_id, "Busy task panicked");
                            self.drain_into_idle(&inbox);
                        }
                    }
                }
            }
        }
    }

    /// idle 到期（`Decision::Done` 后）守卫：章节非空（turn_count > 0）则重开——
    /// 先尝试 收尾 留存（成败均开新章节，见 docs/topics/session.md「章节重开」）。
    fn should_wrap_up(&self) -> bool {
        self.conversation.has_unwrapped_content()
    }

    /// context_tokens 阈值路径——上下文超限（防幻觉/超模型窗口）时空闲即触发，
    /// 优先于 inbox/idle 检查。失败也重开（context_tokens 清零，不会立即再触发）。
    fn should_wrap_up_by_tokens(&self) -> bool {
        let threshold = self.engine.app.cfg.agent.context.compact_token_threshold;
        threshold > 0 && self.conversation.context_tokens() >= threshold
    }

    fn start_wrap_up(&mut self, inbox: &Inbox) {
        let ctx = self.build_turn_context(WakeEvents::new(vec![]));
        let (handle, result_rx) = self.runtime.spawn_wrap_up(
            ctx,
            self.conversation.messages_for_wrap_up(),
            inbox.clone(),
        );
        self.state = SessionState::Busy {
            handle,
            result_rx,
            started_at: tokio::time::Instant::now(),
        };
    }

    async fn idle_tick(
        &mut self,
        inbox: &Inbox,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) -> NextStep {
        if self.should_wrap_up_by_tokens() {
            return NextStep::WrapUp;
        }
        // 章节重开（恢复超期）：恢复的会话上次活动已超过 idle_timeout → 立即重开，
        // 不等新的 idle（先 收尾 留存，成败都开干净章节）；未超期则清除标记走正常链路
        if let Some(last_active) = self.conversation.take_restored_last_active()
            && self.conversation.has_unwrapped_content()
        {
            let timeout = jiff::SignedDuration::from_secs(self.idle_timeout().as_secs() as i64);
            if jiff::Timestamp::now().duration_since(last_active) > timeout {
                return NextStep::WrapUp;
            }
        }
        let timeout = self.idle_timeout();
        let deadline = self.schedule.next_deadline(timeout);

        tokio::select! {
            _ = inbox.notified() => {},
            Some(query) = status_rx.recv() => return NextStep::Status(query),
            _ = if let Some(t) = deadline {
                futures::future::Either::Left(tokio::time::sleep_until(t))
            } else {
                futures::future::Either::Right(futures::future::pending())
            } => {},
            else => return NextStep::Exit,
        }

        let events = inbox.drain();
        self.schedule.enqueue(events);

        match self.schedule.decide(timeout) {
            Decision::Ready(events) => NextStep::Activate(events),
            Decision::Defer => NextStep::Idle,
            Decision::Done => {
                if self.should_wrap_up() {
                    NextStep::WrapUp
                } else {
                    NextStep::Exit
                }
            }
        }
    }

    async fn on_wrap_up_done(&mut self, wrap_up: String) {
        let wrapped_up_steps = self.conversation.step_count();
        self.engine.app.event_bus.emit(
            self.chat_id,
            AgentEventPayload::WrapUpCompleted {
                step_count: wrapped_up_steps as usize,
                summary: wrap_up.clone(),
            },
        );
        // 重开章节：收尾 摘要置于新章节开头；章节整体替换（turn_count 归零）——
        // 事件编号按章节重来，见 topics/session.md「ContextMeta」
        self.conversation.start_new_chapter(Some(wrap_up));

        let snap = self.conversation.snapshot();
        if let Err(e) = self
            .engine
            .app
            .db
            .srv
            .conversation
            .save(&snap, self.chat_id)
            .await
        {
            tracing::warn!(%self.chat_id, "Failed to save 收尾: {e}");
        }
    }

    /// Busy 收尾统一回 Idle：drain inbox 入队，不退出。
    /// 退出只发生在 idle_tick 的 `Decision::Done` + 章节空。
    fn drain_into_idle(&mut self, inbox: &Inbox) {
        let events = inbox.drain();
        self.schedule.enqueue(events);
        self.state = SessionState::Idle;
    }
}

// ── Status ────────────────────────────────────────────────────────────────

impl AgentSession {
    pub fn build_status(&mut self) -> proxy::SessionStatus {
        let (turn_in_progress, turn_elapsed) = match &self.state {
            SessionState::Busy { started_at, .. } => {
                (true, Some(started_at.elapsed().as_secs_f64()))
            }
            SessionState::Idle => (false, None),
        };
        proxy::SessionStatus {
            scheduler: self.schedule.snapshot(),
            step_count: self.conversation.step_count() as usize,
            context_tokens: self.conversation.context_tokens(),
            conversation_msgs: self.conversation.message_count(),
            turn_in_progress,
            turn_elapsed_secs: turn_elapsed,
            model: self.engine.app.cfg.agent.model.clone(),
        }
    }

    pub fn answer_status(&mut self, query: oneshot::Sender<proxy::SessionStatus>) {
        let _ = query.send(self.build_status());
    }
}
