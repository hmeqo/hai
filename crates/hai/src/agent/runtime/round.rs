use autoagents::core::tool::ToolCallResult;

#[derive(Clone)]
pub struct Round {
    pub segment: String,
    pub tool_calls: Vec<ToolCallResult>,
    /// 本轮处理后最后一条消息的 ID，下一轮作为 `since_id` 传入
    pub since_id: i64,
}

#[derive(Clone)]
pub struct RoundTaskPayload {
    pub prompt: String,
    pub segment: String,
    pub message_ids: Vec<i64>,
    pub since_id: i64,
}
