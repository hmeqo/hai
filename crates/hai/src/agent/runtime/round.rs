use autoagents::core::tool::ToolCallResult;
use uuid::Uuid;

#[derive(Clone)]
pub struct Round {
    pub segment: String,
    pub tool_calls: Vec<ToolCallResult>,
    /// LLM 最后一轮的文本回复（可能为空）
    pub response: String,
    /// 本轮处理后最后一条消息的 ID，下一轮作为 `since_id` 传入
    pub since_id: i64,
    /// 本轮已展示给 agent 的记忆和话题 ID，后续轮 dedup 用
    pub shown_memory_ids: Vec<Uuid>,
    pub shown_topic_ids: Vec<Uuid>,
}

#[derive(Clone)]
pub struct RoundTaskPayload {
    pub prompt: String,
    pub segment: String,
    pub message_ids: Vec<i64>,
    pub since_id: i64,
    pub shown_memory_ids: Vec<Uuid>,
    pub shown_topic_ids: Vec<Uuid>,
}
