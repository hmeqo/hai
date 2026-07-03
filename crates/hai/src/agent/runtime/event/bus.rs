use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::domain::vo::ChatId;

// ── AgentEvent ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum AgentEvent {
    SessionCreated {
        chat_id: ChatId,
        mode: String,
        model: String,
    },
    SessionDone {
        chat_id: ChatId,
    },
    TurnStarted {
        chat_id: ChatId,
        turn: usize,
        reason: String,
    },
    ContextBuilt {
        chat_id: ChatId,
        turn: usize,
        msg_count: usize,
        full_prompt: String,
    },
    ToolCall {
        chat_id: ChatId,
        turn: usize,
        tool: String,
        args: String,
    },
    ToolCallResult {
        chat_id: ChatId,
        turn: usize,
        tool: String,
        summary: String,
        success: bool,
    },
    TurnCompleted {
        chat_id: ChatId,
        turn: usize,
        tool_calls: usize,
        elapsed_ms: u64,
        prompt_tokens: u32,
        completion_tokens: u32,
        has_spoken: bool,
        response: String,
        reasoning: Option<String>,
    },
}

impl AgentEvent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionCreated { .. } => "session_created",
            Self::SessionDone { .. } => "session_done",
            Self::TurnStarted { .. } => "turn_started",
            Self::ContextBuilt { .. } => "context_built",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolCallResult { .. } => "tool_call_result",
            Self::TurnCompleted { .. } => "turn_completed",
        }
    }

    pub fn chat_id(&self) -> Option<i64> {
        match self {
            Self::SessionCreated { chat_id, .. }
            | Self::SessionDone { chat_id }
            | Self::TurnStarted { chat_id, .. }
            | Self::ContextBuilt { chat_id, .. }
            | Self::ToolCall { chat_id, .. }
            | Self::ToolCallResult { chat_id, .. }
            | Self::TurnCompleted { chat_id, .. } => Some(chat_id.0),
        }
    }

    pub fn payload(&self) -> Value {
        match self {
            Self::SessionCreated { mode, model, .. } => json!({
                "mode": mode, "model": model
            }),
            Self::SessionDone { .. } => json!({}),
            Self::TurnStarted { turn, reason, .. } => json!({
                "turn": turn, "reason": reason
            }),
            Self::ContextBuilt { turn, msg_count, full_prompt, .. } => json!({
                "turn": turn, "msg_count": msg_count, "full_prompt": full_prompt
            }),
            Self::ToolCall { turn, tool, args, .. } => json!({
                "turn": turn, "tool": tool, "args": args
            }),
            Self::ToolCallResult { turn, tool, summary, success, .. } => json!({
                "turn": turn, "tool": tool, "summary": summary, "success": success
            }),
            Self::TurnCompleted { turn, tool_calls, elapsed_ms, prompt_tokens, completion_tokens, has_spoken, response, reasoning, .. } => json!({
                "turn": turn, "tool_calls": tool_calls, "elapsed_ms": elapsed_ms,
                "prompt_tokens": prompt_tokens, "completion_tokens": completion_tokens,
                "has_spoken": has_spoken, "response": response,
                "reasoning": reasoning
            }),
        }
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
            kind: event.as_str().to_string(),
            chat_id: event.chat_id(),
            payload: toasty::Json(event.payload()),
        })
        .exec(&mut db.clone())
        .await
        {
            tracing::warn!(kind = %event.as_str(), error = %e, "Failed to flush agent event");
        }
    }
}
