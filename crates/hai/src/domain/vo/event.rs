use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::ChatId;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum AgentEventPayload {
    SessionCreated {
        chat_id: ChatId,
        mode: String,
        model: String,
    },
    SessionDone {
        chat_id: ChatId,
    },
    #[serde(alias = "turn_started")]
    WakeStarted {
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
    RunCompleted {
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
    ModelRetry {
        chat_id: ChatId,
        turn: usize,
        reason: ModelRetryReason,
    },
    Preempted {
        chat_id: ChatId,
        turn: usize,
    },
    RunFailed {
        chat_id: ChatId,
        turn: usize,
        elapsed_ms: u64,
        error: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Display, IntoStaticStr, EnumString)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ModelRetryReason {
    TextWithoutTool,
    TimeoutRetry,
}
