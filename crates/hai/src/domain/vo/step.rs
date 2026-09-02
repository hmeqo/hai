use serde::{Deserialize, Serialize};

use crate::domain::vo::ToolCallResult;

/// 一次 LLM exec_chat 的完整记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    pub tool_calls: Vec<ToolCallResult>,
    pub response: String,
    pub reasoning: Option<String>,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}
