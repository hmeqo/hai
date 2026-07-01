mod main;
pub mod multimodal;

use std::sync::Arc;

use async_trait::async_trait;
use genai::chat::ChatMessage;
pub use main::MainAgent;
pub use multimodal::{MediaInput, MediaSource, ModelService, MultimodalService};

use crate::agentcore::tool::{AgentTool, ToolError};

#[derive(Debug)]
pub struct AgentOutput {
    pub tool_calls: Vec<super::runtime::round::ToolCallResult>,
    pub messages: Vec<ChatMessage>,
    /// ReAct 循环结束时的最终输出文本。
    pub final_response: String,
}

#[async_trait]
pub trait AgentNode: Send + Sync {
    fn name(&self) -> &str;
    async fn run(
        &self,
        messages: Vec<ChatMessage>,
        tools: Vec<Arc<dyn AgentTool>>,
    ) -> Result<AgentOutput, ToolError>;
}
