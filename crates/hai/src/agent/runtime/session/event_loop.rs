//! Event loop + 状态类型 + build_status

use tokio::sync::{mpsc, oneshot};

use super::{
    super::event::{Inbox, WakeEvents},
    AgentSession, SessionState, proxy,
    scheduler::Decision,
};
use crate::{
    agent::runtime::types::{BusySignal, RunOutput},
    domain::vo::AgentEventPayload,
};

// ── Busy Outcome ──────────────────────────────────────────────────────────

enum BusyOutcome {
    Status(oneshot::Sender<proxy::SessionStatus>),
    Success(RunOutput),
    CompactDone(String),
    Failed,
    Cancelled,
}

// ── Next Step ─────────────────────────────────────────────────────────────

enum NextStep {
    Status(oneshot::Sender<proxy::SessionStatus>),
    Activate(WakeEvents),
    Compact,
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
                    NextStep::Compact => {
                        let (handle, result_rx) = self
                            .runtime
                            .spawn_compact(self.conversation.messages_for_compact());
                        self.state = SessionState::Busy {
                            handle,
                            result_rx,
                            started_at: tokio::time::Instant::now(),
                        };
                    }
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
                            Ok(BusySignal::Run(output)) => BusyOutcome::Success(output),
                            Ok(BusySignal::Compact(text)) => BusyOutcome::CompactDone(text),
                            Ok(BusySignal::Failed) => BusyOutcome::Failed,
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
                        BusyOutcome::CompactDone(text) => {
                            self.on_compact_done(text).await;
                            if self.drain_into_idle_or_exit(&inbox) {
                                break;
                            }
                        }
                        BusyOutcome::Failed => {
                            tracing::warn!(%self.chat_id, "Busy task failed");
                            if self.drain_into_idle_or_exit(&inbox) {
                                break;
                            }
                        }
                        BusyOutcome::Cancelled => {
                            tracing::warn!(%self.chat_id, "Busy task panicked");
                            if self.drain_into_idle_or_exit(&inbox) {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }

    fn should_compact(&self) -> bool {
        self.run_count > 0
    }

    async fn idle_tick(
        &mut self,
        inbox: &Inbox,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) -> NextStep {
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
                if self.should_compact() {
                    NextStep::Compact
                } else {
                    NextStep::Exit
                }
            }
        }
    }

    async fn on_compact_done(&mut self, compact: String) {
        self.conversation.open_new_chapter(compact);
        self.run_count = 0;
        self.engine.app.event_bus.emit(
            self.chat_id,
            AgentEventPayload::CompactCompleted {
                run_count: self.run_count,
            },
        );

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
            tracing::warn!(%self.chat_id, "Failed to save compact: {e}");
        }
    }

    fn drain_into_idle_or_exit(&mut self, inbox: &Inbox) -> bool {
        let events = inbox.drain();
        if events.is_empty() {
            return true;
        }
        self.schedule.enqueue(events);
        self.state = SessionState::Idle;
        false
    }
}

// ── Status ────────────────────────────────────────────────────────────────

impl AgentSession {
    pub fn build_status(&mut self) -> proxy::SessionStatus {
        let (run_in_progress, run_elapsed) = match &self.state {
            SessionState::Busy { started_at, .. } => {
                (true, Some(started_at.elapsed().as_secs_f64()))
            }
            SessionState::Idle => (false, None),
        };
        proxy::SessionStatus {
            scheduler: self.schedule.snapshot(),
            turns_count: self.conversation.turn_count(),
            context_tokens: self.conversation.context_tokens(),
            conversation_msgs: self.conversation.message_count(),
            run_in_progress,
            run_elapsed_secs: run_elapsed,
            model: self.engine.app.cfg.agent.model.clone(),
        }
    }

    pub fn answer_status(&mut self, query: oneshot::Sender<proxy::SessionStatus>) {
        let _ = query.send(self.build_status());
    }
}
