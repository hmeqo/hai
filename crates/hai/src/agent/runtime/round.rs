use genai::chat::ChatMessage;
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

#[derive(Clone)]
pub struct Round {
    pub messages: Vec<ChatMessage>,
    pub tool_calls: Vec<ToolCallResult>,
    /// 本轮处理后最后一条消息的 ID，下一轮作为 `since_id` 传入
    pub since_id: i64,
    /// 本轮已展示给 agent 的记忆和话题 ID，后续轮 dedup 用
    pub shown_memory_ids: Vec<Uuid>,
    pub shown_topic_ids: Vec<Uuid>,
}

impl Round {
    pub fn sent_message(&self) -> bool {
        self.tool_calls
            .iter()
            .any(|t| matches!(t.tool_name.as_str(), "send_message" | "send_voice"))
    }
}

#[derive(Clone)]
pub struct RoundTaskPayload {
    pub messages: Vec<ChatMessage>,
    pub prompt: String,
    pub message_ids: Vec<i64>,
    pub since_id: i64,
    pub shown_memory_ids: Vec<Uuid>,
    pub shown_topic_ids: Vec<Uuid>,
}
