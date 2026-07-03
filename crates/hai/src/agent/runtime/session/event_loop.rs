//! Event loop + 状态类型 + build_status

use tokio::{
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::Instant,
};

use super::{
    super::event::WakeEvent,
    AgentSession, SessionState, proxy, scheduler::Decision,
};
use crate::agent::runtime::{types::{Inbox, ProcessingOutput}, AgentEvent};

// ── Active Processing ─────────────────────────────────────────────────────

pub(super) struct ActiveProcessing {
    #[allow(dead_code)]
    pub(super) handle: JoinHandle<()>,
    pub(super) result_rx: oneshot::Receiver<proxy::ProcessingSignal>,
    pub(super) started_at: Instant,
}

impl ActiveProcessing {
    pub(super) async fn await_completion(
        &mut self,
        status_rx: &mut mpsc::UnboundedReceiver<oneshot::Sender<proxy::SessionStatus>>,
    ) -> ProcessingOutcome {
        tokio::select! {
            biased;
            Some(query) = status_rx.recv() => ProcessingOutcome::Status(query),
            result = &mut self.result_rx => match result {
                Ok(Some(output)) => ProcessingOutcome::Success(output),
                Ok(None) => ProcessingOutcome::Failed,
                Err(_) => ProcessingOutcome::Cancelled,
            },
        }
    }
}

// ── Processing Outcome ────────────────────────────────────────────────────

pub(super) enum ProcessingOutcome {
    Status(oneshot::Sender<proxy::SessionStatus>),
    Success(ProcessingOutput),
    Failed,
    Cancelled,
}

// ── Next Step ─────────────────────────────────────────────────────────────

enum NextStep {
    Status(oneshot::Sender<proxy::SessionStatus>),
    Activate(Vec<WakeEvent>),
    Idle,
    Done,
    Closed,
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
                    NextStep::Done | NextStep::Closed => break,
                },
                SessionState::Active(mut active) => {
                    let outcome = active.await_completion(&mut status_rx).await;
                    match outcome {
                        ProcessingOutcome::Status(query) => {
                            self.answer_status(query);
                            self.state = SessionState::Active(active);
                        }
                        ProcessingOutcome::Success(output) => {
                            self.on_complete(output, &inbox).await;
                        }
                        ProcessingOutcome::Failed => {
                            tracing::warn!(
                                %self.chat_id,
                                elapsed_secs = %active.started_at.elapsed().as_secs_f64(),
                                "Processing failed",
                            );
                            let events = inbox.drain();
                            self.schedule.enqueue(events);
                            self.state = SessionState::Idle;
                        }
                        ProcessingOutcome::Cancelled => {
                            tracing::warn!(
                                %self.chat_id,
                                elapsed_secs = %active.started_at.elapsed().as_secs_f64(),
                                "Processing panicked",
                            );
                            let events = inbox.drain();
                            self.schedule.enqueue(events);
                            self.state = SessionState::Idle;
                        }
                    }
                }
            }
        }
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
            else => return NextStep::Closed,
        }

        let events = inbox.drain();
        self.schedule.enqueue(events);

        match self.schedule.decide(timeout) {
            Decision::Ready(events) => NextStep::Activate(events),
            Decision::Defer => NextStep::Idle,
            Decision::Done => {
                self.engine.app.event_bus.emit(AgentEvent::SessionDone {
                    chat_id: self.chat_id,
                });
                NextStep::Done
            }
        }
    }
}

// ── Status ────────────────────────────────────────────────────────────────

impl AgentSession {
    pub(super) fn build_status(&mut self) -> super::proxy::SessionStatus {
        let (run_in_progress, run_elapsed) = match &self.state {
            SessionState::Active(active) => (true, Some(active.started_at.elapsed().as_secs_f64())),
            SessionState::Idle => (false, None),
        };
        let (turns_count, last_turns) = {
            let c = &self.conversation;
            (c.turn_count(), Some(c.last_turns.clone()))
        };
        super::proxy::SessionStatus {
            scheduler: self.schedule.snapshot(),
            turns_count,
            prompt_tokens: self.conversation.prompt_tokens,
            conversation_msgs: self.conversation.messages.len(),
            mode: self.conversation.mode.label(),
            run_in_progress,
            run_elapsed_secs: run_elapsed,
            model: self.engine.app.cfg.agent.model.clone(),
            last_turns,
        }
    }

    pub(super) fn answer_status(&mut self, query: oneshot::Sender<super::proxy::SessionStatus>) {
        let _ = query.send(self.build_status());
    }
}
