use genai::chat::ChatMessage;
pub use genai::chat::Usage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// 工具调用的执行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool_name: String,
    pub success: bool,
    pub arguments: Value,
    pub result: Value,
}

impl ToolCallResult {
    pub fn ok(tool_name: impl Into<String>, args: Value, result: Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: true,
            arguments: args,
            result,
        }
    }

    pub fn err(tool_name: impl Into<String>, args: Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: false,
            arguments: args,
            result: Value::Null,
        }
    }
}

/// 一次 exec_chat 的完整记录。
#[derive(Clone)]
#[allow(dead_code)]
pub struct Turn {
    pub tool_calls: Vec<ToolCallResult>,
    pub usage: Usage,
    pub stop_reason: String,
}

/// 一次 dispatch 的输出。
#[derive(Clone)]
pub struct Run {
    pub turns: Vec<Turn>,
    /// 本轮处理后最后一条消息的 ID，下一轮作为 `since_id` 传入
    pub since_id: i64,
    /// 本轮已展示给 agent 的记忆和话题 ID，后续轮 dedup 用
    pub shown_memory_ids: Vec<Uuid>,
    pub shown_topic_ids: Vec<Uuid>,
}

impl Run {
    pub fn tool_calls(&self) -> impl Iterator<Item = &ToolCallResult> {
        self.turns.iter().flat_map(|t| t.tool_calls.iter())
    }

    pub fn sent_message(&self) -> bool {
        self.tool_calls()
            .any(|t| matches!(t.tool_name.as_str(), "send_message" | "send_voice"))
    }
}

/// oneshot 通道传输的完整输出（含消息历史）。
pub struct RunOutput {
    pub run: Run,
    pub messages: Vec<ChatMessage>,
}

#[derive(Clone)]
pub struct RunPayload {
    pub messages: Vec<ChatMessage>,
    pub prompt: String,
    pub message_ids: Vec<i64>,
    pub since_id: i64,
    pub shown_memory_ids: Vec<Uuid>,
    pub shown_topic_ids: Vec<Uuid>,
}
