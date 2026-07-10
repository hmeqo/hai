use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::ChatId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnOutput {
    pub run: usize,
    pub turn: usize,
    pub reasoning: Option<String>,
    pub response: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    pub chat_id: ChatId,
    pub payload: AgentEventPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "kebab-case")]
pub enum AgentEventPayload {
    RunStarted {
        run: usize,
        reason: String,
        msg_count: usize,
        full_prompt: String,
    },

    ToolCall {
        run: usize,
        tool: String,
        args: String,
    },
    ToolCallResult {
        run: usize,
        tool: String,
        summary: String,
        success: bool,
    },

    TurnCompleted(TurnOutput),

    RunCompleted {
        output: TurnOutput,
        tool_calls: usize,
        elapsed_ms: u64,
        prompt_tokens: u32,
        completion_tokens: u32,
        has_spoken: bool,
    },

    ModelRetry {
        run: usize,
        reason: ModelRetryReason,
    },
    Preempted {
        run: usize,
        count: usize,
        reasons: String,
        content: String,
    },
    RunFailed {
        run: usize,
        elapsed_ms: u64,
        error: String,
    },
}

impl AgentEventPayload {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::RunStarted { .. } => "run_started",
            Self::ToolCall { .. } => "tool_call",
            Self::ToolCallResult { .. } => "tool_call_result",
            Self::TurnCompleted(..) => "turn_completed",
            Self::RunCompleted { .. } => "run_completed",
            Self::ModelRetry { .. } => "model_retry",
            Self::Preempted { .. } => "preempted",
            Self::RunFailed { .. } => "run_failed",
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Display, IntoStaticStr, EnumString,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ModelRetryReason {
    ResponseWithText,
    TimeoutRetry,
}
