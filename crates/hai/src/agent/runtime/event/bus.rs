use std::time::Duration;

use serde_json::Value;
use tokio::sync::mpsc;

pub use crate::domain::vo::AgentEventPayload as AgentEvent;

// ── AgentEvent ─────────────────────────────────────────────────────────────────

impl AgentEvent {
    pub fn chat_id(&self) -> Option<i64> {
        match self {
            Self::SessionCreated { chat_id, .. }
            | Self::SessionDone { chat_id }
            | Self::WakeStarted { chat_id, .. }
            | Self::ContextBuilt { chat_id, .. }
            | Self::ToolCall { chat_id, .. }
            | Self::ToolCallResult { chat_id, .. }
            | Self::TurnCompleted { chat_id, .. } => Some(chat_id.0),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::SessionCreated { .. } => "session_created",
            Self::SessionDone { .. } => "session_done",
            Self::WakeStarted { .. } => "wake_started",
            Self::ContextBuilt { .. } => "context_built",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolCallResult { .. } => "tool_call_result",
            Self::TurnCompleted { .. } => "turn_completed",
        }
    }

    pub fn payload(&self) -> Value {
        serde_json::to_value(self).unwrap()
    }
}

// ── AgentEventBus ──────────────────────────────────────────────────────────────

const FLUSH_INTERVAL: Duration = Duration::from_millis(200);
const FLUSH_BATCH: usize = 50;

#[derive(Clone)]
pub struct AgentEventBus {
    tx: mpsc::UnboundedSender<AgentEvent>,
}

impl AgentEventBus {
    pub fn new(db: toasty::Db) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(collector(rx, db));
        Self { tx }
    }

    pub fn emit(&self, event: AgentEvent) {
        let _ = self.tx.send(event);
    }
}

async fn collector(mut rx: mpsc::UnboundedReceiver<AgentEvent>, db: toasty::Db) {
    let mut batch = Vec::with_capacity(FLUSH_BATCH);
    loop {
        tokio::select! {
            Some(event) = rx.recv() => {
                batch.push(event);
                if batch.len() >= FLUSH_BATCH {
                    flush(&batch, &db).await;
                    batch.clear();
                }
            }
            _ = tokio::time::sleep(FLUSH_INTERVAL) => {
                if !batch.is_empty() {
                    flush(&batch, &db).await;
                    batch.clear();
                }
            }
            else => break,
        }
    }
}

async fn flush(batch: &[AgentEvent], db: &toasty::Db) {
    use crate::domain::model::Event;

    for event in batch {
        if let Err(e) = toasty::create!(Event {
            domain: "agent".to_string(),
            payload: toasty::Json(event.payload()),
        })
        .exec(&mut db.clone())
        .await
        {
            tracing::warn!(kind = event.kind(), error = %e, "Failed to flush agent event");
        }
    }
}
